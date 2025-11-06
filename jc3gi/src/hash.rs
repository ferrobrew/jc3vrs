#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
unsafe fn hashlittle_impl(data: *const u8, len: u64, seed: u32) -> u64 {
    unsafe {
        let f: unsafe extern "system" fn(data: *const u8, len: u64, seed: u32) -> u64 = ::std::mem::transmute(
            0x144A8CB00 as usize,
        );
        f(data, len, seed)
    }
}
pub fn hashlittle(data: &[u8]) -> u64 {
    unsafe { hashlittle_impl(data.as_ptr(), data.len() as u64, 0) }
}
