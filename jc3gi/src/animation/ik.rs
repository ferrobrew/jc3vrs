#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CHumanIK {}
impl CHumanIK {
    pub const AddEffectorTargetPosition_ADDRESS: usize = 0x140408860;
    pub unsafe fn AddEffectorTargetPosition(
        &mut self,
        effector: i32,
        pos: *const crate::types::math::Vector3,
        solve_step: i32,
        pass_info: i32,
        effector_interpolation: bool,
        effector_interpolation_rate: f32,
        effector_blend_out: bool,
        effector_blend_out_rate: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                effector: i32,
                pos: *const crate::types::math::Vector3,
                solve_step: i32,
                pass_info: i32,
                effector_interpolation: bool,
                effector_interpolation_rate: f32,
                effector_blend_out: bool,
                effector_blend_out_rate: f32,
            ) = ::std::mem::transmute(Self::AddEffectorTargetPosition_ADDRESS);
            f(
                self as *mut Self as _,
                effector,
                pos,
                solve_step,
                pass_info,
                effector_interpolation,
                effector_interpolation_rate,
                effector_blend_out,
                effector_blend_out_rate,
            )
        }
    }
}
impl std::convert::AsRef<CHumanIK> for CHumanIK {
    fn as_ref(&self) -> &CHumanIK {
        self
    }
}
impl std::convert::AsMut<CHumanIK> for CHumanIK {
    fn as_mut(&mut self) -> &mut CHumanIK {
        self
    }
}
pub const NHandIKTask_Update_ADDRESS: usize = 0x140816430;
unsafe fn NHandIKTask_Update(
    ctx: *mut crate::state::SStateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::SStateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NHandIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
