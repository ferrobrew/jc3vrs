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
/// The engine's 4x4 matrix, in the D3D convention: row-major storage with row vectors (a point is a
/// row vector left-multiplied by the matrix, `clip = p * M`).
///
/// The 16 floats are four rows; a world or camera transform's rows are its basis vectors:
/// `data[0..2]` is right (+X), `data[4..6]` is up (+Y), `data[8..10]` is the +Z basis (so forward is
/// `-data[8..10]`), and `data[12..14]` is the translation. Right-handed, Y-up.
///
/// The glam conversions below bridge to glam's column-vector convention by transposing (row-major rows
/// become glam columns), so glam matrix math on a converted [`Matrix4`] works without an explicit
/// transpose. To build an engine transform in glam, set the basis vectors as the glam *columns*
/// (right, up, -forward, translation) and use `to_cols_array`.
pub struct Matrix4 {
    pub data: [f32; 16],
}
fn _Matrix4_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], Matrix4>([0u8; 0x40]);
    }
    unreachable!()
}
impl Matrix4 {
    pub const Multiply4x4_ADDRESS: usize = 0x140034530;
    /// The engine's 4x4 product, `result = a * b` (row-major: result row `i` is `a`'s row `i` times
    /// `b`). For example, `Multiply4x4(View, Proj, ViewProjection)` gives `VP = View * Proj` and
    /// `clip = p * View * Proj`. Static.
    pub unsafe fn Multiply4x4(
        a: *const crate::types::math::Matrix4,
        b: *const crate::types::math::Matrix4,
        result: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                a: *const crate::types::math::Matrix4,
                b: *const crate::types::math::Matrix4,
                result: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::Multiply4x4_ADDRESS);
            f(a, b, result)
        }
    }
    pub const PerspectiveFovInverse_ADDRESS: usize = 0x1400390E0;
    /// Builds the inverse of a symmetric perspective projection into `self` (also returned), from a
    /// vertical field of view, an aspect ratio, and the far and near planes. The screen-space
    /// reconstruction passes (SSR, deferred clustered lighting, SSAO, screen-space subsurface,
    /// atmospheric scattering, depth of field) call it to recover a clip-to-view basis, then multiply
    /// by the render context's camera transform to reach clip-to-world. The result is purely diagonal
    /// (`[0][0] = aspect·tan(fov/2)`, `[1][1] = tan(fov/2)`, plus standard-depth `z` terms) and so
    /// cannot represent an off-center (asymmetric) frustum.
    pub unsafe fn PerspectiveFovInverse(
        &mut self,
        fov: f32,
        aspect: f32,
        far: f32,
        near: f32,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                fov: f32,
                aspect: f32,
                far: f32,
                near: f32,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::PerspectiveFovInverse_ADDRESS,
            );
            f(self as *mut Self as _, fov, aspect, far, near)
        }
    }
}
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
#[cfg(feature = "glam")]
impl From<glam::Mat3> for Matrix3 {
    fn from(m: glam::Mat3) -> Self {
        Self { data: m.to_cols_array() }
    }
}
#[cfg(feature = "glam")]
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
#[cfg(feature = "glam")]
impl From<glam::Mat4> for Matrix4 {
    fn from(m: glam::Mat4) -> Self {
        Self { data: m.to_cols_array() }
    }
}
#[cfg(feature = "glam")]
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
#[cfg(feature = "glam")]
impl std::ops::Mul for Matrix4 {
    type Output = Matrix4;
    fn mul(self, rhs: Matrix4) -> Matrix4 {
        Matrix4::from(glam::Mat4::from(rhs) * glam::Mat4::from(self))
    }
}
#[cfg(all(test, feature = "glam"))]
#[allow(clippy::items_after_test_module)]
mod matrix4_mul_tests {
    use super::*;
    fn translation(x: f32, y: f32, z: f32) -> Matrix4 {
        let mut m = Matrix4 {
            data: [
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ],
        };
        m.data[12] = x;
        m.data[13] = y;
        m.data[14] = z;
        m
    }
    #[test]
    fn mul_matches_engine_convention() {
        let id = translation(0.0, 0.0, 0.0);
        let a = translation(1.0, 2.0, 3.0);
        assert_eq!((id * a).data, a.data);
        assert_eq!((a * id).data, a.data);
        let ab = a * translation(10.0, 20.0, 30.0);
        assert!((ab.data[12] - 11.0).abs() < 1e-5);
        assert!((ab.data[13] - 22.0).abs() < 1e-5);
        assert!((ab.data[14] - 33.0).abs() < 1e-5);
    }
}
#[cfg(feature = "glam")]
impl From<glam::Vec2> for Vector2 {
    fn from(v: glam::Vec2) -> Self {
        Self { data: [v.x, v.y] }
    }
}
#[cfg(feature = "glam")]
impl From<Vector2> for glam::Vec2 {
    fn from(v: Vector2) -> Self {
        glam::Vec2::new(v.data[0], v.data[1])
    }
}
#[cfg(feature = "glam")]
impl From<glam::Vec3> for Vector3 {
    fn from(v: glam::Vec3) -> Self {
        Self { data: [v.x, v.y, v.z] }
    }
}
#[cfg(feature = "glam")]
impl From<Vector3> for glam::Vec3 {
    fn from(v: Vector3) -> Self {
        glam::Vec3::new(v.data[0], v.data[1], v.data[2])
    }
}
#[cfg(feature = "glam")]
impl From<glam::Vec4> for Vector4 {
    fn from(v: glam::Vec4) -> Self {
        Self { data: [v.x, v.y, v.z, v.w] }
    }
}
#[cfg(feature = "glam")]
impl From<Vector4> for glam::Vec4 {
    fn from(v: Vector4) -> Self {
        glam::Vec4::new(v.data[0], v.data[1], v.data[2], v.data[3])
    }
}
