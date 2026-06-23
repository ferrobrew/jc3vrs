#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The opaque per-task state-machine context passed to character and task update functions. Used only
/// behind pointers; the layout is not yet mapped.
pub struct StateContext {}
impl StateContext {}
impl std::convert::AsRef<StateContext> for StateContext {
    fn as_ref(&self) -> &StateContext {
        self
    }
}
impl std::convert::AsMut<StateContext> for StateContext {
    fn as_mut(&mut self) -> &mut StateContext {
        self
    }
}
