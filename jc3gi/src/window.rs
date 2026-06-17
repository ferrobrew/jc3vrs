#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
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
        ) -> i64 = ::std::mem::transmute(0x140006F40 as usize);
        f(hwnd, msg, wparam, lparam)
    }
}
