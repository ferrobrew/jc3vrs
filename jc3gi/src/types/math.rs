#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Matrix3 {
    pub data: [f32; 9],
}
fn _Matrix3_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x24], Matrix3>([0u8; 0x24]);
    }
    unreachable!()
}
impl Matrix3 {}
impl std::convert::AsRef<Matrix3> for Matrix3 {
    fn as_ref(&self) -> &Matrix3 {
        self
    }
}
impl std::convert::AsMut<Matrix3> for Matrix3 {
    fn as_mut(&mut self) -> &mut Matrix3 {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Matrix4 {
    pub data: [f32; 16],
}
fn _Matrix4_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], Matrix4>([0u8; 0x40]);
    }
    unreachable!()
}
impl Matrix4 {}
impl std::convert::AsRef<Matrix4> for Matrix4 {
    fn as_ref(&self) -> &Matrix4 {
        self
    }
}
impl std::convert::AsMut<Matrix4> for Matrix4 {
    fn as_mut(&mut self) -> &mut Matrix4 {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Plane {
    pub normal: crate::types::math::Vector3,
    pub distance: f32,
}
fn _Plane_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], Plane>([0u8; 0x10]);
    }
    unreachable!()
}
impl Plane {}
impl std::convert::AsRef<Plane> for Plane {
    fn as_ref(&self) -> &Plane {
        self
    }
}
impl std::convert::AsMut<Plane> for Plane {
    fn as_mut(&mut self) -> &mut Plane {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Vector2 {
    pub data: [f32; 2],
}
fn _Vector2_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], Vector2>([0u8; 0x8]);
    }
    unreachable!()
}
impl Vector2 {}
impl std::convert::AsRef<Vector2> for Vector2 {
    fn as_ref(&self) -> &Vector2 {
        self
    }
}
impl std::convert::AsMut<Vector2> for Vector2 {
    fn as_mut(&mut self) -> &mut Vector2 {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Vector3 {
    pub data: [f32; 3],
}
fn _Vector3_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], Vector3>([0u8; 0xC]);
    }
    unreachable!()
}
impl Vector3 {}
impl std::convert::AsRef<Vector3> for Vector3 {
    fn as_ref(&self) -> &Vector3 {
        self
    }
}
impl std::convert::AsMut<Vector3> for Vector3 {
    fn as_mut(&mut self) -> &mut Vector3 {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(4))]
pub struct Vector4 {
    pub data: [f32; 4],
}
fn _Vector4_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], Vector4>([0u8; 0x10]);
    }
    unreachable!()
}
impl Vector4 {}
impl std::convert::AsRef<Vector4> for Vector4 {
    fn as_ref(&self) -> &Vector4 {
        self
    }
}
impl std::convert::AsMut<Vector4> for Vector4 {
    fn as_mut(&mut self) -> &mut Vector4 {
        self
    }
}
impl From<glam::Mat3> for Matrix3 {
    fn from(m: glam::Mat3) -> Self {
        Self { data: m.to_cols_array() }
    }
}
impl From<Matrix3> for glam::Mat3 {
    fn from(m: Matrix3) -> Self {
        glam::Mat3::from_cols_array(&m.data)
    }
}
impl Matrix3 {
    pub fn as_ptr(&self) -> *const f32 {
        self.data.as_ptr()
    }
    pub fn as_mut_ptr(&mut self) -> *mut f32 {
        self.data.as_mut_ptr()
    }
}
impl From<glam::Mat4> for Matrix4 {
    fn from(m: glam::Mat4) -> Self {
        Self { data: m.to_cols_array() }
    }
}
impl From<Matrix4> for glam::Mat4 {
    fn from(m: Matrix4) -> Self {
        glam::Mat4::from_cols_array(&m.data)
    }
}
impl Matrix4 {
    pub fn as_ptr(&self) -> *const f32 {
        self.data.as_ptr()
    }
    pub fn as_mut_ptr(&mut self) -> *mut f32 {
        self.data.as_mut_ptr()
    }
}
impl From<glam::Vec2> for Vector2 {
    fn from(v: glam::Vec2) -> Self {
        Self { data: [v.x, v.y] }
    }
}
impl From<Vector2> for glam::Vec2 {
    fn from(v: Vector2) -> Self {
        glam::Vec2::new(v.data[0], v.data[1])
    }
}
impl From<glam::Vec3> for Vector3 {
    fn from(v: glam::Vec3) -> Self {
        Self { data: [v.x, v.y, v.z] }
    }
}
impl From<Vector3> for glam::Vec3 {
    fn from(v: Vector3) -> Self {
        glam::Vec3::new(v.data[0], v.data[1], v.data[2])
    }
}
impl From<glam::Vec4> for Vector4 {
    fn from(v: glam::Vec4) -> Self {
        Self { data: [v.x, v.y, v.z, v.w] }
    }
}
impl From<Vector4> for glam::Vec4 {
    fn from(v: Vector4) -> Self {
        glam::Vec4::new(v.data[0], v.data[1], v.data[2], v.data[3])
    }
}
