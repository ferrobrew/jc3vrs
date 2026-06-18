#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
pub const hashlittle_impl_ADDRESS: usize = 0x141119880;
unsafe fn hashlittle_impl(data: *const u8, len: u64, seed: u32) -> u64 {
    unsafe {
        let f: unsafe extern "system" fn(data: *const u8, len: u64, seed: u32) -> u64 = ::std::mem::transmute(
            hashlittle_impl_ADDRESS,
        );
        f(data, len, seed)
    }
}
pub fn hashlittle(data: &[u8]) -> u64 {
    unsafe { hashlittle_impl(data.as_ptr(), data.len() as u64, 0) }
}
