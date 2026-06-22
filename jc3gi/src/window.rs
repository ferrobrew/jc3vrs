#![cfg_attr(any(), rustfmt::skip)]
pub const WndProc_ADDRESS: usize = 0x140006F40;
/// The game's Win32 window procedure: LRESULT WndProc(HWND, UINT msg, WPARAM, LPARAM).
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
