#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Opaque per-task state-machine context passed to character/task Update functions
/// (SStateContext). Layout TBD; used behind pointers.
pub struct SStateContext {}
impl SStateContext {}
impl std::convert::AsRef<SStateContext> for SStateContext {
    fn as_ref(&self) -> &SStateContext {
        self
    }
}
impl std::convert::AsMut<SStateContext> for SStateContext {
    fn as_mut(&mut self) -> &mut SStateContext {
        self
    }
}
