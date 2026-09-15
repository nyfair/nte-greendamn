use core::ffi::c_void;

pub type BOOL = i32;
pub type DWORD = u32;
pub type HANDLE = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type SIZE_T = usize;
pub type LPCVOID = *const c_void;
pub type LPVOID = *mut c_void;

pub const TRUE: BOOL = 1;
pub const DLL_PROCESS_ATTACH: u32 = 1;

pub const GENERIC_WRITE: DWORD = 0x4000_0000;
pub const FILE_SHARE_READ: DWORD = 0x0000_0001;
pub const FILE_SHARE_WRITE: DWORD = 0x0000_0002;
pub const CREATE_ALWAYS: DWORD = 2;
pub const FILE_ATTRIBUTE_NORMAL: DWORD = 0x80;
pub const INVALID_HANDLE_VALUE: isize = -1;
pub const PAGE_EXECUTE_READWRITE: DWORD = 0x40;
pub const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: DWORD = 0x0000_0004;
pub const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: DWORD = 0x0000_0002;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct SystemTime {
    pub year: u16,
    pub month: u16,
    pub day_of_week: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millisecond: u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    pub fn GetModuleHandleExW(flags: DWORD, name_or_addr: *const u16, out: *mut HINSTANCE) -> BOOL;
    pub fn GetModuleFileNameW(module: HINSTANCE, buf: *mut u16, size: DWORD) -> DWORD;
    pub fn GetCurrentProcess() -> HANDLE;
    pub fn CloseHandle(handle: HANDLE) -> BOOL;
    pub fn DisableThreadLibraryCalls(module: HINSTANCE) -> BOOL;
    pub fn Sleep(ms: DWORD);

    pub fn VirtualProtect(addr: LPVOID, size: SIZE_T, new: DWORD, old: *mut DWORD) -> BOOL;
    pub fn FlushInstructionCache(process: HANDLE, base: LPCVOID, size: SIZE_T) -> BOOL;

    pub fn ReadProcessMemory(
        process: HANDLE,
        base: LPCVOID,
        buffer: LPVOID,
        size: SIZE_T,
        read: *mut SIZE_T,
    ) -> BOOL;

    pub fn WriteProcessMemory(
        process: HANDLE,
        base: LPVOID,
        buffer: LPCVOID,
        size: SIZE_T,
        written: *mut SIZE_T,
    ) -> BOOL;

    pub fn CreateThread(
        attrs: LPVOID,
        stack_size: SIZE_T,
        start: extern "system" fn(LPVOID) -> DWORD,
        param: LPVOID,
        flags: DWORD,
        thread_id: *mut DWORD,
    ) -> HANDLE;

    pub fn CreateFileW(
        name: *const u16,
        access: DWORD,
        share: DWORD,
        sa: LPVOID,
        creation: DWORD,
        flags: DWORD,
        template: HANDLE,
    ) -> HANDLE;

    pub fn WriteFile(
        file: HANDLE,
        buffer: LPCVOID,
        bytes: DWORD,
        written: *mut DWORD,
        overlapped: LPVOID,
    ) -> BOOL;

    pub fn GetLocalTime(out: *mut SystemTime);

    pub fn GetTickCount64() -> u64;
}
