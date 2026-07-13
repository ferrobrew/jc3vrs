#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A hashed string id (`CHashString` / `ava::idstring`): a string reduced to its little-endian
/// `hashlittle` hash, used as a type, state, and event id throughout the animation and event
/// systems. Only the hash survives in the release build.
pub struct HashString {
    /// The `hashlittle(name)` hash identifying the string.
    pub m_Hash: u32,
}
fn _HashString_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], HashString>([0u8; 0x4]);
    }
    unreachable!()
}
impl HashString {}
impl std::convert::AsRef<HashString> for HashString {
    fn as_ref(&self) -> &HashString {
        self
    }
}
impl std::convert::AsMut<HashString> for HashString {
    fn as_mut(&mut self) -> &mut HashString {
        self
    }
}
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
