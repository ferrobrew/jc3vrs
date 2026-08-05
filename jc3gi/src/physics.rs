#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CPfxCharacterInstance`: the character's physics-side body instance, owned by `CCharacter`
/// through a `boost::shared_ptr`. Only the members needed so far are bound.
pub struct PfxCharacterInstance {}
impl PfxCharacterInstance {
    pub const SetOrientation_ADDRESS: usize = 0x140239100;
    /// Sets the physics body's rotation from the rotation part of `m` (via a quaternion
    /// conversion) and zeroes the body's angular velocity; the translation part is ignored. The
    /// swim movement cores ([`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) and
    /// [`UpdateUnderwaterMovement`](crate::input::swim::UpdateUnderwaterMovement)) apply their computed
    /// body orientation through this each update.
    pub unsafe fn SetOrientation(&mut self, m: *const crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                m: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::SetOrientation_ADDRESS);
            f(self as *mut Self as _, m)
        }
    }
    pub const GetVelocity_ADDRESS: usize = 0x14024D820;
    /// Returns the physics body's current linear velocity (world space, 3 floats) via the body's
    /// Havok transform/velocity chain.
    pub unsafe fn GetVelocity(&self, out: *mut crate::types::math::Vector3) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::GetVelocity_ADDRESS);
            f(self as *const Self as _, out)
        }
    }
}
impl std::convert::AsRef<PfxCharacterInstance> for PfxCharacterInstance {
    fn as_ref(&self) -> &PfxCharacterInstance {
        self
    }
}
impl std::convert::AsMut<PfxCharacterInstance> for PfxCharacterInstance {
    fn as_mut(&mut self) -> &mut PfxCharacterInstance {
        self
    }
}
pub const DoRotate_ADDRESS: usize = 0x1407754F0;
/// Rotates `from` toward `to` along the shortest arc, writing the result to `out`. It takes the
/// rotation from `from` to `to` as an angle-axis pair and applies
/// `min(angle / 0.2617994, 1) * rate` of it, so the step eases out as the two directions converge.
/// Note that the step is **not** clamped to the remaining angle: a `rate` above 0.2617994 radians
/// (15 degrees) overshoots the target, and one above twice that diverges. It logs `"UHOH!"` when
/// the two directions are more than about 162 degrees apart, where the rotation axis is
/// ill-conditioned.
///
/// The swim locomotion family calls it from two places per frame, for two different quantities:
///
/// - [`UpdateSurfaceMovement`](crate::input::swim::UpdateSurfaceMovement) and
///   [`UpdateUnderwaterMovement`](crate::input::swim::UpdateUnderwaterMovement) rotate the character's
///   current forward toward the desired direction (the world move direction from
///   [`ControllerUtility::TransformInputDirToWorldDir`](crate::input::controller_utility::ControllerUtility::TransformInputDirToWorldDir)
///   while moving, the aim-target direction while aiming, the grapple target while reeling, or the
///   current forward when the stick is neutral). The result is the `direction` argument to
///   `CMatrix4f::CreateOrientation`, so **this call is what turns the body**.
/// - [`ProcessMotion`](crate::physics::ProcessMotion) slerps the character's current velocity direction toward the wanted one.
pub unsafe fn DoRotate(
    from: *const crate::types::math::Vector3,
    to: *const crate::types::math::Vector3,
    rate: f32,
    out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            from: *const crate::types::math::Vector3,
            to: *const crate::types::math::Vector3,
            rate: f32,
            out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(DoRotate_ADDRESS);
        f(from, to, rate, out)
    }
}
pub const ProcessMotion_ADDRESS: usize = 0x140775760;
/// Turns a wanted velocity into the velocity actually applied this frame: it slerps the direction
/// of `current_vel` toward the direction of `wanted_vel` through [`DoRotate`](crate::physics::DoRotate), limited to
/// `slerp_delta_angle` radians, then rescales the result to the horizontal magnitude of
/// `wanted_vel`. Either direction falls back to the character's world-matrix forward when it is
/// degenerate, and `xz_only` flattens both onto the ground plane first. `dt` is unused: the
/// magnitude comes from `wanted_vel`, not from integration. The surface swim movement core uses it
/// to keep a swimmer's motion from snapping direction when the stick swings.
pub unsafe fn ProcessMotion(
    character: *mut crate::character::character::Character,
    dt: f32,
    current_vel: *const crate::types::math::Vector3,
    wanted_vel: *const crate::types::math::Vector3,
    slerp_delta_angle: f32,
    xz_only: bool,
    vel_out: *mut crate::types::math::Vector3,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            current_vel: *const crate::types::math::Vector3,
            wanted_vel: *const crate::types::math::Vector3,
            slerp_delta_angle: f32,
            xz_only: bool,
            vel_out: *mut crate::types::math::Vector3,
        ) = ::std::mem::transmute(ProcessMotion_ADDRESS);
        f(character, dt, current_vel, wanted_vel, slerp_delta_angle, xz_only, vel_out)
    }
}
