#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
pub const SetListenerTransform_ADDRESS: usize = 0x140D2AFF0;
/// Sets the 3D audio listener pose and velocity. Free function.
unsafe fn SetListenerTransform(
    mat: *const crate::types::math::Matrix4,
    velocity: *const crate::types::math::Vector3,
    enable: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            mat: *const crate::types::math::Matrix4,
            velocity: *const crate::types::math::Vector3,
            enable: bool,
        ) = ::std::mem::transmute(SetListenerTransform_ADDRESS);
        f(mat, velocity, enable)
    }
}
