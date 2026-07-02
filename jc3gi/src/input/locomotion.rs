#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::character::character::AimState;
#[repr(C, align(8))]
pub struct InstanceProperties {}
impl InstanceProperties {}
impl std::convert::AsRef<InstanceProperties> for InstanceProperties {
    fn as_ref(&self) -> &InstanceProperties {
        self
    }
}
impl std::convert::AsMut<InstanceProperties> for InstanceProperties {
    fn as_mut(&mut self) -> &mut InstanceProperties {
        self
    }
}
#[repr(C, align(8))]
/// The locomotion move task active while the character is aim-relative (strafing). Mirrors
/// [`NStateTask_InputLocoMoveTask`] but stays in the aim-relative acts as long as an aim bit is set.
pub struct NStateTask_InputLocoAimRelativeTask {}
impl NStateTask_InputLocoAimRelativeTask {
    pub const Update_ADDRESS: usize = 0x140836500;
    /// The per-frame update. Like [`NStateTask_InputLocoMoveTask::Update`] it re-reads
    /// [`Character::m_AimFlags`] each frame and dispatches to the strafe or run acts, so dropping the
    /// aim bits here falls back to run/steer.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, p2)
        }
    }
}
impl std::convert::AsRef<NStateTask_InputLocoAimRelativeTask>
for NStateTask_InputLocoAimRelativeTask {
    fn as_ref(&self) -> &NStateTask_InputLocoAimRelativeTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputLocoAimRelativeTask>
for NStateTask_InputLocoAimRelativeTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputLocoAimRelativeTask {
        self
    }
}
#[repr(C, align(8))]
/// The locomotion move task active when the character is on foot and *not* holding an aim. Its
/// [`Update`](NStateTask_InputLocoMoveTask::Update) reads [`Character::m_AimFlags`] and dispatches
/// to run/steer versus strafe locomotion.
pub struct NStateTask_InputLocoMoveTask {}
impl NStateTask_InputLocoMoveTask {
    pub const Update_ADDRESS: usize = 0x1408125E0;
    /// The per-frame update. Reads the blackboard move direction and speed, then branches on
    /// [`Character::m_AimFlags`] ([`m_AimingWeapon`](character::character::AimState::m_AimingWeapon) /
    /// [`m_AimingGrapple`](character::character::AimState::m_AimingGrapple)): with neither set it queues
    /// [`QueueMoveActions`](NStateTask_LocoUtil::QueueMoveActions) (run — the body steers toward the
    /// movement vector), otherwise it queues the aim-relative (strafe) acts. This branch is the
    /// concrete point where on-foot movement becomes third-person steer instead of FPS-style strafe.
    pub unsafe fn Update(
        ctx: *mut crate::state::StateContext,
        p1: *mut ::std::ffi::c_void,
        p2: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::state::StateContext,
                p1: *mut ::std::ffi::c_void,
                p2: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(ctx, p1, p2)
        }
    }
}
impl std::convert::AsRef<NStateTask_InputLocoMoveTask> for NStateTask_InputLocoMoveTask {
    fn as_ref(&self) -> &NStateTask_InputLocoMoveTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputLocoMoveTask> for NStateTask_InputLocoMoveTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputLocoMoveTask {
        self
    }
}
#[repr(C, align(8))]
/// The locomotion task that reads the movement effectors and writes the move direction to the
/// character blackboard.
pub struct NStateTask_InputLocoSetTargetDirTask {}
impl NStateTask_InputLocoSetTargetDirTask {
    pub const SetupTargetDir_ADDRESS: usize = 0x14081E130;
    /// Reads the move effectors via the action-map effector lookup, rotates the raw vector
    /// camera-relative, and writes both the raw and camera-relative move directions to the character
    /// blackboard.
    pub unsafe fn SetupTargetDir(
        &mut self,
        character: *mut crate::character::character::Character,
        props: *mut crate::input::locomotion::InstanceProperties,
    ) -> f64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                props: *mut crate::input::locomotion::InstanceProperties,
            ) -> f64 = ::std::mem::transmute(Self::SetupTargetDir_ADDRESS);
            f(self as *mut Self as _, character, props)
        }
    }
}
impl std::convert::AsRef<NStateTask_InputLocoSetTargetDirTask>
for NStateTask_InputLocoSetTargetDirTask {
    fn as_ref(&self) -> &NStateTask_InputLocoSetTargetDirTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_InputLocoSetTargetDirTask>
for NStateTask_InputLocoSetTargetDirTask {
    fn as_mut(&mut self) -> &mut NStateTask_InputLocoSetTargetDirTask {
        self
    }
}
#[repr(C, align(8))]
/// Free helpers that queue the concrete locomotion animation acts. The queued act encodes the
/// movement style: [`QueueMoveActions`](NStateTask_LocoUtil::QueueMoveActions) steers the body
/// toward the movement direction, while the aim-relative variants keep the body facing the aim and
/// play directional strafe animations.
pub struct NStateTask_LocoUtil {}
impl NStateTask_LocoUtil {
    pub const QueueMoveActions_ADDRESS: usize = 0x140832C00;
    /// Queues the run/steer acts (`ACT_MOVE_NO_AIM` and friends). The body turns to face the
    /// movement direction, so directional input rotates the whole character — the third-person
    /// on-foot behavior that reads as a "tank turn" in VR.
    pub unsafe fn QueueMoveActions(
        character: *mut crate::character::character::Character,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
            ) -> bool = ::std::mem::transmute(Self::QueueMoveActions_ADDRESS);
            f(character)
        }
    }
    pub const QueueAimRelativeMoveActions_ADDRESS: usize = 0x1408326B0;
    /// Queues the aim-relative (strafe) acts. It derives the facing reference from the weapon-aim
    /// target when [`m_AimingWeapon`](character::character::AimState::m_AimingWeapon) is set (or the
    /// grapple target for [`m_AimingGrapple`](character::character::AimState::m_AimingGrapple)),
    /// and falls back to the character's own current forward when neither is set — so this alone
    /// does not turn the body toward the camera without an aim reference.
    pub unsafe fn QueueAimRelativeMoveActions(
        character: *mut crate::character::character::Character,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
            ) -> bool = ::std::mem::transmute(Self::QueueAimRelativeMoveActions_ADDRESS);
            f(character)
        }
    }
    pub const QueueTransitionToAimRelativeMoveActions_ADDRESS: usize = 0x1408321A0;
    /// Queues the one-shot transition into the aim-relative (strafe) acts, played on the frame the
    /// character starts aiming.
    pub unsafe fn QueueTransitionToAimRelativeMoveActions(
        character: *mut crate::character::character::Character,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
            ) -> bool = ::std::mem::transmute(
                Self::QueueTransitionToAimRelativeMoveActions_ADDRESS,
            );
            f(character)
        }
    }
}
impl std::convert::AsRef<NStateTask_LocoUtil> for NStateTask_LocoUtil {
    fn as_ref(&self) -> &NStateTask_LocoUtil {
        self
    }
}
impl std::convert::AsMut<NStateTask_LocoUtil> for NStateTask_LocoUtil {
    fn as_mut(&mut self) -> &mut NStateTask_LocoUtil {
        self
    }
}
