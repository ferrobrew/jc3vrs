#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
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
        ) = ::std::mem::transmute(0x140D2AFF0 as usize);
        f(mat, velocity, enable)
    }
}
