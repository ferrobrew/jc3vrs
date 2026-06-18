#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CPlayerAimControl {}
impl CPlayerAimControl {
    pub const GetAdjustedCameraMatrix_ADDRESS: usize = 0x140C3E510;
    pub unsafe fn GetAdjustedCameraMatrix(
        result: *mut crate::types::math::Matrix4,
        weapon: *mut crate::aim::aim::CWeaponBase,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                result: *mut crate::types::math::Matrix4,
                weapon: *mut crate::aim::aim::CWeaponBase,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::GetAdjustedCameraMatrix_ADDRESS,
            );
            f(result, weapon)
        }
    }
    pub const UpdateDirectAim_ADDRESS: usize = 0x140CE5350;
    pub unsafe fn UpdateDirectAim(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateDirectAim_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<CPlayerAimControl> for CPlayerAimControl {
    fn as_ref(&self) -> &CPlayerAimControl {
        self
    }
}
impl std::convert::AsMut<CPlayerAimControl> for CPlayerAimControl {
    fn as_mut(&mut self) -> &mut CPlayerAimControl {
        self
    }
}
#[repr(C, align(8))]
pub struct CWeaponBase {}
impl CWeaponBase {
    pub const GetGripPosition_ADDRESS: usize = 0x140966840;
    pub unsafe fn GetGripPosition(
        &self,
        hand: i32,
        matrix: *mut crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                hand: i32,
                matrix: *mut crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::GetGripPosition_ADDRESS);
            f(self as *const Self as _, hand, matrix)
        }
    }
}
impl std::convert::AsRef<CWeaponBase> for CWeaponBase {
    fn as_ref(&self) -> &CWeaponBase {
        self
    }
}
impl std::convert::AsMut<CWeaponBase> for CWeaponBase {
    fn as_mut(&mut self) -> &mut CWeaponBase {
        self
    }
}
pub const NAutoAimToTarget_Update_ADDRESS: usize = 0x140809C60;
unsafe fn NAutoAimToTarget_Update(
    ctx: *mut crate::state::SStateContext,
    p: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::SStateContext,
            p: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NAutoAimToTarget_Update_ADDRESS);
        f(ctx, p)
    }
}
