#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Player aim control; in normal third-person play the aim is camera-derived.
pub struct PlayerAimControl {}
impl PlayerAimControl {
    pub const GetAdjustedCameraMatrix_ADDRESS: usize = 0x140C3E510;
    /// Returns the matrix used by the aim raycast: the ADS branch reads the alternate-aim transform,
    /// otherwise the camera transform; `weapon` adds optional ballistic pitch. Static method that
    /// returns CMatrix4f by value (sret: `result` out-param, also returned in rax).
    pub unsafe fn GetAdjustedCameraMatrix(
        result: *mut crate::types::math::Matrix4,
        weapon: *mut crate::aim::aim::WeaponBase,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                result: *mut crate::types::math::Matrix4,
                weapon: *mut crate::aim::aim::WeaponBase,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::GetAdjustedCameraMatrix_ADDRESS,
            );
            f(result, weapon)
        }
    }
    pub const UpdateDirectAim_ADDRESS: usize = 0x140CE5350;
    /// Raycasts from the camera position along camera-forward to determine the aim target.
    pub unsafe fn UpdateDirectAim(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateDirectAim_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<PlayerAimControl> for PlayerAimControl {
    fn as_ref(&self) -> &PlayerAimControl {
        self
    }
}
impl std::convert::AsMut<PlayerAimControl> for PlayerAimControl {
    fn as_mut(&mut self) -> &mut PlayerAimControl {
        self
    }
}
#[repr(C, align(8))]
pub struct WeaponBase {}
impl WeaponBase {
    pub const GetGripPosition_ADDRESS: usize = 0x140966840;
    /// Returns the grip transform for the given hand (E_WEAPON_GRIP_HAND_*, modeled as i32).
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
impl std::convert::AsRef<WeaponBase> for WeaponBase {
    fn as_ref(&self) -> &WeaponBase {
        self
    }
}
impl std::convert::AsMut<WeaponBase> for WeaponBase {
    fn as_mut(&mut self) -> &mut WeaponBase {
        self
    }
}
pub const NAutoAimToTarget_Update_ADDRESS: usize = 0x140809C60;
/// Character-state task that overrides the aim direction toward a locked target. Free function.
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
