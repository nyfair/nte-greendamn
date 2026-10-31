#![no_std]
#![allow(non_camel_case_types, non_snake_case, unsafe_op_in_unsafe_fn)]

mod xx;
mod bypass;
mod uncensor;

use core::ffi::c_void;
use core::panic::PanicInfo;

use xx::buf::Buf;
use xx::ffi;
use xx::log;
use xx::mem;

const TARGET_NAME: &str = "HTGame.exe";
const PLUGIN_NAME: &str = "nte-greendamn";
const BUILD: &str = "2026-10-31";
const TARGET_MODULE: &[u8] = TARGET_NAME.as_bytes();
const LOG_NAME: &[u8] = "nte-greendamn.log".as_bytes();
const TICK_MS: u32 = 2_500;
const HEARTBEAT_MS: u64 = 60_000;

#[panic_handler]
fn on_panic(_info: &PanicInfo) -> ! {
    let mut b = Buf::new();
    b.push_str(PLUGIN_NAME);
    b.push_str(" panic!");
    log::log_buf(&b);
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

fn run() {
    if !log::init() {
        return;
    }
    let module = match wait_for_module(TARGET_MODULE) {
        Some(m) => m,
        None => {
            let mut b = Buf::new();
            b.push_str("Not loaded by ");
            b.push_str(TARGET_NAME);
            log::log_buf(&b);
            return;
        }
    };

    let mut b = Buf::new();
    b.push_str(TARGET_NAME);
    b.push_str(" @ 0x");
    b.push_hex(module.base as u64, 0);
    b.push_str(" size=0x");
    b.push_hex(module.size as u64, 0);
    log::log_buf(&b);
    let mut b = Buf::new();
    b.push_str(PLUGIN_NAME);
    b.push_str(" build ");
    b.push_str(BUILD);
    log::log_buf(&b);

    // Enable UE mod support.
    bypass::universal(&module);
    let mut last_report = unsafe { ffi::GetTickCount64() };
    #[cfg(feature = "measure")]
    uncensor::extra_init();

    loop {
        let now = unsafe { ffi::GetTickCount64() };
        uncensor::tick(&module);
        if now.wrapping_sub(last_report) >= HEARTBEAT_MS {
            last_report = now;
            let mut b = Buf::new();
            b.push_str("patches ");
            b.push_u64(uncensor::patches());
            b.push_str(", repatches ");
            b.push_u64(uncensor::repatches());
            b.push_str(", manager 0x");
            b.push_hex(uncensor::manager() as u64, 0);
            log::log_buf(&b);
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
