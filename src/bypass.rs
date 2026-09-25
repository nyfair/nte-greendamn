use core::cmp::min;

use crate::buf::Buf;
use crate::ffi;
use crate::log;
use crate::mem::*;

/// mov al, 1; ret - makes the target function always return true
const PATCH: [u8; 3] = [0xB0, 0x01, 0xC3];

const MAX_PAT: usize = 96;
const CHUNK: usize = 0x1000;

/// https://github.com/rm-NoobInCoding/UniversalSigBypasser/blob/master/UniversalSigBypasser/dllmain.cpp
const SIG: &str = "48 8D ?? ?? ?? ?? ?? E9 ?? ?? ?? ?? CC CC CC CC 48 83 EC 28 33 D2 48 8D 4C 24 30 E8 ?? ?? ?? ?? 48 8B C8 E8 ?? ?? ?? ?? 48 89 ?? ?? ?? ?? ?? 48 83 C4 28 C3 CC CC CC CC CC CC CC CC CC CC CC CC 48 8D ?? ?? ?? ?? ?? E9";
/// Offset from the hit address to the jump instruction
const SIG_AT: usize = 0x47;

/// Parsed signature: bit `i` of `wild` marks byte `i` as a wildcard
struct Pattern {
    bytes: [u8; MAX_PAT],
    wild: u128,
    len: usize,
}

impl Pattern {
    fn parse(ida: &str) -> Pattern {
        let mut p = Pattern {
            bytes: [0; MAX_PAT],
            wild: 0,
            len: 0,
        };
        for tok in ida.split_ascii_whitespace() {
            if p.len == MAX_PAT {
                break;
            }
            if tok.as_bytes()[0] == b'?' {
                p.wild |= 1 << p.len;
            } else {
                p.bytes[p.len] = u8::from_str_radix(tok, 16).unwrap_or(0);
            }
            p.len += 1;
        }
        p
    }

    fn hit(&self, at: &[u8]) -> bool {
        let mut i = 0;
        while i < self.len {
            if self.wild & (1 << i) == 0 && self.bytes[i] != at[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Walks the module image in chunks, is caught by the trailing `len - 1` bytes kept at the head of `win`
fn find(module: &Module, ida: &str) -> Option<usize> {
    let p = Pattern::parse(ida);
    if p.len == 0 || p.len > module.size {
        return None;
    }

    let end = module.end();
    let (first, wild_first) = (p.bytes[0], p.wild & 1 != 0);
    let mut win = [0u8; CHUNK + MAX_PAT];
    let mut carry = 0usize;
    let mut addr = module.base;

    while addr < end {
        let want = min(CHUNK, end - addr);
        let got = unsafe { read_raw(addr, win[carry..].as_mut_ptr(), want) };
        if got == 0 {
            // unreadable page (gap between sections): skip it
            addr += want;
            carry = 0;
            continue;
        }

        let have = carry + got;
        let mut i = 0usize;
        while i + p.len <= have {
            if (wild_first || win[i] == first) && p.hit(&win[i..]) {
                return Some(addr - carry + i);
            }
            i += 1;
        }

        carry = min(p.len - 1, have);
        win.copy_within(have - carry..have, 0);
        addr += got;
    }
    None
}

/// Reads the jump at `at` (E9 jmp / E8 call) and computes its destination
fn follow_jump(at: usize) -> Option<usize> {
    let mut op = 0u8;
    if unsafe { read_raw(at, &mut op, 1) } != 1 || (op != 0xE9 && op != 0xE8) {
        return None;
    }
    let rel = read_u32(at + 1)? as i32;
    Some(at.wrapping_add(5).wrapping_add_signed(rel as isize))
}

fn patch(at: usize) -> bool {
    let mut old: ffi::DWORD = 0;
    unsafe {
        let size = PATCH.len();
        if ffi::VirtualProtect(at as ffi::LPVOID, size, ffi::PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        core::ptr::copy_nonoverlapping(PATCH.as_ptr(), at as *mut u8, size);
        ffi::FlushInstructionCache(ffi::GetCurrentProcess(), at as ffi::LPCVOID, size);
        let mut tmp: ffi::DWORD = 0;
        ffi::VirtualProtect(at as ffi::LPVOID, size, old, &mut tmp);
    }
    true
}

fn read_back(at: usize) -> [u8; PATCH.len()] {
    let mut b = [0u8; PATCH.len()];
    unsafe { read_raw(at, b.as_mut_ptr(), b.len()) };
    b
}

/// Logs `prefix` + a hex value + `suffix`
fn log_hex(prefix: &str, v: u64, suffix: &str) {
    let mut b = Buf::new();
    b.push_str(prefix);
    b.push_hex(v, 0);
    b.push_str(suffix);
    log::log_buf(&b);
}

/// Finds the signature, follows the jmp/call behind it to the target function and rewrites that function to always return true
pub fn universal(module: &Module) {
    let Some(hit) = find(module, SIG) else {
        log::line("bypass: signature not found, skipped");
        return;
    };

    let target = follow_jump(hit + SIG_AT);

    let mut b = Buf::new();
    b.push_str("bypass: hit 0x");
    b.push_hex(hit as u64, 0);
    b.push_str(", offset +0x");
    b.push_hex(SIG_AT as u64, 0);
    match target {
        Some(t) => {
            b.push_str(", jump target 0x");
            b.push_hex(t as u64, 0);
        }
        None => b.push_str(", not E9/E8 at that address"),
    }
    log::log_buf(&b);

    let Some(target) = target else {
        return;
    };
    if !module.contains(target) {
        log_hex("bypass: jump target 0x", target as u64, " is outside HTGame.exe, giving up");
        return;
    }

    let wrote = patch(target);
    log_hex(
        if !wrote {
            "bypass: VirtualProtect failed @0x"
        } else if read_back(target) == PATCH {
            "bypass: wrote B0 01 C3, read-back matches @0x"
        } else {
            "bypass: read-back mismatch after write @0x"
        },
        target as u64,
        "",
    );
}
