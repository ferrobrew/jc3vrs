#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A keyed value store attached to game objects; the character's instance lives at
/// `Character + 0x2060` and carries the locomotion values (the camera-relative move direction, the
/// target face direction, the up direction). Values are read and written by id through the
/// template accessors below.
pub struct ObjectBlackboard {}
impl ObjectBlackboard {
    pub const GetVector3_ADDRESS: usize = 0x1405C5AC0;
    /// The `GetBlackboardValue<CVector3f>` template instantiation. Returns whether the id was
    /// present; the value is written through `value`.
    pub unsafe fn GetVector3(
        &mut self,
        id: u32,
        value: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(Self::GetVector3_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const GetFloat_ADDRESS: usize = 0x1405C5BA0;
    /// The `GetBlackboardValue<float>` template instantiation.
    pub unsafe fn GetFloat(&mut self, id: u32, value: *mut f32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *mut f32,
            ) -> bool = ::std::mem::transmute(Self::GetFloat_ADDRESS);
            f(self as *mut Self as _, id, value)
        }
    }
    pub const SetVector3_ADDRESS: usize = 0x1405C6A70;
    /// The `SetBlackboardValue<CVector3f>` template instantiation. `value` is by-value `CVector3f`
    /// at the C++ level, which the MSVC x64 ABI passes by reference — hence the pointer here.
    /// `flags` is `1` at every observed call site; `name` is an optional debug label, null in
    /// release calls.
    pub unsafe fn SetVector3(
        &mut self,
        id: u32,
        value: *const crate::types::math::Vector3,
        flags: u32,
        name: *const u8,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                id: u32,
                value: *const crate::types::math::Vector3,
                flags: u32,
                name: *const u8,
            ) -> bool = ::std::mem::transmute(Self::SetVector3_ADDRESS);
            f(self as *mut Self as _, id, value, flags, name)
        }
    }
}
impl std::convert::AsRef<ObjectBlackboard> for ObjectBlackboard {
    fn as_ref(&self) -> &ObjectBlackboard {
        self
    }
}
impl std::convert::AsMut<ObjectBlackboard> for ObjectBlackboard {
    fn as_mut(&mut self) -> &mut ObjectBlackboard {
        self
    }
}
