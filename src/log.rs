use core::sync::atomic::{AtomicIsize, Ordering};

use crate::buf::Buf;
use crate::ffi;

static LOG: AtomicIsize = AtomicIsize::new(ffi::INVALID_HANDLE_VALUE);

const LOG_NAME: &[u8] = b"nte-greendamn.log";

pub fn init() -> bool {
    let addr = &LOG as *const AtomicIsize as *const u16;
    let mut hmod: ffi::HINSTANCE = core::ptr::null_mut();
    let ok = unsafe {
        ffi::GetModuleHandleExW(
            ffi::GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | ffi::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            addr,
            &mut hmod,
        )
    };
    if ok == 0 || hmod.is_null() {
        return false;
    }

    let mut path = [0u16; 512];
    let n = unsafe { ffi::GetModuleFileNameW(hmod, path.as_mut_ptr(), 512) };
    if n == 0 {
        return false;
    }
    let used = n as usize;
    let mut dir_len = 0usize;
    let mut i = 0usize;
    while i < used {
        if path[i] == b'\\' as u16 {
            dir_len = i + 1;
        }
        i += 1;
    }

    let mut k = dir_len;
    for &c in LOG_NAME {
        if k + 2 >= path.len() {
            return false;
        }
        path[k] = c as u16;
        k += 1;
    }
    path[k] = 0;

    let h = unsafe {
        ffi::CreateFileW(
            path.as_ptr(),
            ffi::GENERIC_WRITE,
            ffi::FILE_SHARE_READ | ffi::FILE_SHARE_WRITE,
            core::ptr::null_mut(),
            ffi::CREATE_ALWAYS,
            ffi::FILE_ATTRIBUTE_NORMAL,
            core::ptr::null_mut(),
        )
    };
    let raw = h as isize;
    if raw == ffi::INVALID_HANDLE_VALUE || raw == 0 {
        return false;
    }
    LOG.store(raw, Ordering::Release);
    true
}

pub fn line(s: &str) {
    let h = LOG.load(Ordering::Acquire);
    if h == ffi::INVALID_HANDLE_VALUE || h == 0 {
        return;
    }

    let mut b = Buf::new();
    let mut st = ffi::SystemTime::default();
    unsafe { ffi::GetLocalTime(&mut st) };
    b.push_byte(b'[');
    b.push_u64_pad(st.hour as u64, 2);
    b.push_byte(b':');
    b.push_u64_pad(st.minute as u64, 2);
    b.push_byte(b':');
    b.push_u64_pad(st.second as u64, 2);
    b.push_byte(b'.');
    b.push_u64_pad(st.millisecond as u64, 3);
    b.push_str("] ");
    b.push_str(s);
    b.push_bytes(b"\n");

    write_all(h, b.as_bytes());
}

fn write_all(handle: isize, data: &[u8]) {
    let mut off = 0usize;
    while off < data.len() {
        let mut written: ffi::DWORD = 0;
        let chunk = (data.len() - off) as u32;
        let ok = unsafe {
            ffi::WriteFile(
                handle as ffi::HANDLE,
                data[off..].as_ptr() as ffi::LPCVOID,
                chunk,
                &mut written,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 || written == 0 {
            break;
        }
        off += written as usize;
    }
}
