#![no_std]
#![allow(non_camel_case_types, non_snake_case, unsafe_op_in_unsafe_fn)]

mod buf;
mod bypass;
mod ffi;
mod log;
mod mem;
mod uncensor;

use core::ffi::c_void;
use core::panic::PanicInfo;

use buf::Buf;

const TARGET_MODULE: &[u8] = b"HTGame.exe";
const TICK_MS: u32 = 1000;
const HEARTBEAT_MS: u64 = 60_000;
// one camera manager per world: the login screen one and the scene one, then we are done
const DONE_AFTER: u64 = 2;

#[panic_handler]
fn on_panic(_info: &PanicInfo) -> ! {
    log::line("nte-greendamn panic!");
    loop {
        unsafe { ffi::Sleep(HEARTBEAT_MS as u32) };
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    hinst: ffi::HINSTANCE,
    reason: u32,
    _: *mut c_void,
) -> ffi::BOOL {
    if reason == ffi::DLL_PROCESS_ATTACH {
        unsafe {
            ffi::DisableThreadLibraryCalls(hinst);
            let h = ffi::CreateThread(
                core::ptr::null_mut(),
                0,
                worker_thread,
                core::ptr::null_mut(),
                0,
                core::ptr::null_mut(),
            );
            if !h.is_null() {
                ffi::CloseHandle(h);
            }
        }
    }
    ffi::TRUE
}

extern "system" fn worker_thread(_param: *mut c_void) -> ffi::DWORD {
    run();
    0
}

pub fn log_buf(b: &Buf) {
    log::line(unsafe { core::str::from_utf8_unchecked(b.as_bytes()) });
}

fn run() {
    if !log::init() {
        return;
    }
    let module = match wait_for_module(TARGET_MODULE) {
        Some(m) => m,
        None => {
            log::line("Not loaded by HTGame.exe");
            return;
        }
    };

    let mut b = Buf::new();
    b.push_str("HTGame.exe @ 0x");
    b.push_hex(module.base as u64, 0);
    b.push_str(" size=0x");
    b.push_hex(module.size as u64, 0);
    log_buf(&b);

    // disable the check that blocks us, then enter the watch loop
    bypass::universal(&module);

    let mut last_report = unsafe { ffi::GetTickCount64() };
    let mut patched_total: u64 = 0;
    let mut rounds: u64 = 0;

    loop {
        let now = unsafe { ffi::GetTickCount64() };
        patched_total += uncensor::recon(&module) as u64;
        rounds += 1;

        if patched_total >= DONE_AFTER {
            // both managers are handled, there is nothing left to watch for
            let mut b = Buf::new();
            b.push_str("done: ");
            b.push_u64(patched_total);
            b.push_str(" objects rewritten, watch loop stopped");
            log_buf(&b);
            return;
        }

        if now.wrapping_sub(last_report) >= HEARTBEAT_MS {
            last_report = now;
            let mut b = Buf::new();
            b.push_str("watching: ");
            b.push_u64(rounds);
            b.push_str(" rounds scanned, ");
            b.push_u64(patched_total);
            b.push_str(" objects rewritten");
            log_buf(&b);
        }

        unsafe { ffi::Sleep(TICK_MS) };
    }
}

fn wait_for_module(name: &[u8]) -> Option<mem::Module> {
    let mut waited: u32 = 0;
    loop {
        if let Some(m) = mem::Module::find(name) {
            return Some(m);
        }
        if waited >= 30_000 {
            return None;
        }
        unsafe { ffi::Sleep(500) };
        waited += 500;
    }
}
