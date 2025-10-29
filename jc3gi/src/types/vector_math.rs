#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
pub struct AlignedQuat {
    pub data: [f32; 4],
}
fn _AlignedQuat_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], AlignedQuat>([0u8; 0x10]);
    }
    unreachable!()
}
impl AlignedQuat {}
impl std::convert::AsRef<AlignedQuat> for AlignedQuat {
    fn as_ref(&self) -> &AlignedQuat {
        self
    }
}
impl std::convert::AsMut<AlignedQuat> for AlignedQuat {
    fn as_mut(&mut self) -> &mut AlignedQuat {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
pub struct AlignedVector3 {
    pub data: [f32; 3],
    pub _w: f32,
}
fn _AlignedVector3_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], AlignedVector3>([0u8; 0x10]);
    }
    unreachable!()
}
impl AlignedVector3 {}
impl std::convert::AsRef<AlignedVector3> for AlignedVector3 {
    fn as_ref(&self) -> &AlignedVector3 {
        self
    }
}
impl std::convert::AsMut<AlignedVector3> for AlignedVector3 {
    fn as_mut(&mut self) -> &mut AlignedVector3 {
        self
    }
}
