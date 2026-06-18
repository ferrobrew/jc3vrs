#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
pub const WndProc_ADDRESS: usize = 0x140006F40;
unsafe fn WndProc(
    hwnd: *mut ::std::ffi::c_void,
    msg: u32,
    wparam: u64,
    lparam: i64,
) -> i64 {
    unsafe {
        let f: unsafe extern "system" fn(
            hwnd: *mut ::std::ffi::c_void,
            msg: u32,
            wparam: u64,
            lparam: i64,
        ) -> i64 = ::std::mem::transmute(WndProc_ADDRESS);
        f(hwnd, msg, wparam, lparam)
    }
}
