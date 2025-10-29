#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
struct Hashlittle {}
impl Hashlittle {
    unsafe fn hashlittle(data: *const u8, len: u64, seed: u32) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                data: *const u8,
                len: u64,
                seed: u32,
            ) -> u64 = ::std::mem::transmute(0x144A8CB00 as usize);
            f(data, len, seed)
        }
    }
}
impl std::convert::AsRef<Hashlittle> for Hashlittle {
    fn as_ref(&self) -> &Hashlittle {
        self
    }
}
impl std::convert::AsMut<Hashlittle> for Hashlittle {
    fn as_mut(&mut self) -> &mut Hashlittle {
        self
    }
}
pub fn hashlittle(data: &[u8]) -> u64 {
    unsafe { Hashlittle::hashlittle(data.as_ptr(), data.len() as u64, 0) }
}
