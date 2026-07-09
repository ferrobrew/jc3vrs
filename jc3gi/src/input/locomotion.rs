#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `SCharacterMovementSettings`: the per-character movement tuning block. Only passed through
/// opaquely so far; its first float is the walk/run speed threshold the start-act selection
/// compares against.
pub struct CharacterMovementSettings {}
impl CharacterMovementSettings {}
impl std::convert::AsRef<CharacterMovementSettings> for CharacterMovementSettings {
    fn as_ref(&self) -> &CharacterMovementSettings {
        self
    }
}
impl std::convert::AsMut<CharacterMovementSettings> for CharacterMovementSettings {
    fn as_mut(&mut self) -> &mut CharacterMovementSettings {
        self
    }
}
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
pub unsafe fn get_LocoUtil_NoAimStrafeMaxAngle() -> &'static mut f32 {
    unsafe { &mut *(0x142D65F90 as *mut f32) }
}
pub unsafe fn get_LocoUtil_NoAimStrafeMaxAngleAlt() -> &'static mut f32 {
    unsafe { &mut *(0x142D65F94 as *mut f32) }
}
pub unsafe fn get_NCharacter_ActMoveNoAim() -> &'static mut u32 {
    unsafe { &mut *(0x142F2FB64 as *mut u32) }
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
/// character blackboard. Like the other `NStateTask` functions, `self` is the *character* -- the
/// task types are namespaces over free functions that take the character as `this` (the blackboard
/// at `+0x2060` and the body forward at `+0x2850` are read straight off it).
pub struct NStateTask_InputLocoSetTargetDirTask {}
impl NStateTask_InputLocoSetTargetDirTask {
    pub const SetupTargetDir_ADDRESS: usize = 0x14081E130;
    /// Reads the move effectors via the action-map effector lookup, rotates the raw vector
    /// camera-relative (via the camera input matrix), and writes the previous and new move
    /// directions to the character blackboard. `target_dir` is in/out: the previous direction on
    /// entry, the new one on return. Below the input deadzone the *previous* direction (or the
    /// body forward) is re-written -- the blackboard move direction is always a unit vector and
    /// carries no input-held information.
    pub unsafe fn SetupTargetDir(
        &mut self,
        target_dir: *mut crate::types::math::Vector3,
        props: *mut crate::input::locomotion::InstanceProperties,
    ) -> f64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target_dir: *mut crate::types::math::Vector3,
                props: *mut crate::input::locomotion::InstanceProperties,
            ) -> f64 = ::std::mem::transmute(Self::SetupTargetDir_ADDRESS);
            f(self as *mut Self as _, target_dir, props)
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
    /// movement direction, so directional input rotates the whole character (third-person on-foot
    /// steering).
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
    pub const EvaluateCharacterOrientation_ADDRESS: usize = 0x14081F8C0;
    /// Computes the character's new orientation for this frame; the body-yaw executor, called from
    /// [`NStateTask_MovementLocomotionTask::Update`]. With `track_face_dir` clear (and outside a
    /// gating animation segment) the rotation comes from the animation root motion
    /// (`CAnimationControl::GetDeltaRotation`) — the run-mode steer. With it set, the target face
    /// direction is read from the character blackboard (id `736589998`) and the body is yawed
    /// toward it: snapped directly when `snap_to_face_dir` is set (gated on the blend being
    /// finished when `wait_for_blend` is also set), otherwise rate-limited to `max_step_deg`
    /// degrees per call — the aim-mode camera tracking. `max_step_deg` must be positive on the
    /// tracking path (it divides by it). `dt` is only used for the attached-to-vehicle up-vector
    /// lerp.
    pub unsafe fn EvaluateCharacterOrientation(
        out: *mut crate::types::math::Matrix4,
        character: *mut crate::character::character::Character,
        track_face_dir: bool,
        snap_to_face_dir: bool,
        wait_for_blend: bool,
        max_step_deg: f32,
        dt: f32,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                out: *mut crate::types::math::Matrix4,
                character: *mut crate::character::character::Character,
                track_face_dir: bool,
                snap_to_face_dir: bool,
                wait_for_blend: bool,
                max_step_deg: f32,
                dt: f32,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::EvaluateCharacterOrientation_ADDRESS,
            );
            f(
                out,
                character,
                track_face_dir,
                snap_to_face_dir,
                wait_for_blend,
                max_step_deg,
                dt,
            )
        }
    }
    pub const EvaluateCharacterOrientationMS_ADDRESS: usize = 0x14081F490;
    /// The model-space variant of
    /// [`EvaluateCharacterOrientation`](NStateTask_LocoUtil::EvaluateCharacterOrientation), also
    /// called from the movement task and from the grapple reel-in controller. The bools are
    /// believed to match the world-space variant minus the blend gate; unverified.
    pub unsafe fn EvaluateCharacterOrientationMS(
        out: *mut crate::types::math::Matrix4,
        character: *mut crate::character::character::Character,
        track_face_dir: bool,
        snap_to_face_dir: bool,
        max_step_deg: f32,
        dt: f32,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                out: *mut crate::types::math::Matrix4,
                character: *mut crate::character::character::Character,
                track_face_dir: bool,
                snap_to_face_dir: bool,
                max_step_deg: f32,
                dt: f32,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::EvaluateCharacterOrientationMS_ADDRESS,
            );
            f(out, character, track_face_dir, snap_to_face_dir, max_step_deg, dt)
        }
    }
    pub const EvaluateCharacterDisplacement_ADDRESS: usize = 0x14081AB90;
    /// Computes this frame's movement direction from the animation root motion and the new body
    /// orientation. Called from [`NStateTask_MovementLocomotionTask::Update`], which normalizes
    /// `out_direction`, scales it by
    /// [`EvaluateCharacterSpeed`](NStateTask_LocoUtil::EvaluateCharacterSpeed), and writes the
    /// result as the character's physics velocity -- so overriding `out_direction` after the call
    /// redirects the movement without touching the speed envelope. The bools and `out_secondary`
    /// are unmapped.
    pub unsafe fn EvaluateCharacterDisplacement(
        character: *mut crate::character::character::Character,
        orientation: *const crate::types::math::Matrix4,
        p3: bool,
        p4: bool,
        p5: bool,
        p6: f32,
        out_secondary: *mut crate::types::math::Vector3,
        out_direction: *mut crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                orientation: *const crate::types::math::Matrix4,
                p3: bool,
                p4: bool,
                p5: bool,
                p6: f32,
                out_secondary: *mut crate::types::math::Vector3,
                out_direction: *mut crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::EvaluateCharacterDisplacement_ADDRESS);
            f(character, orientation, p3, p4, p5, p6, out_secondary, out_direction)
        }
    }
    pub const EvaluateCharacterSpeed_ADDRESS: usize = 0x14081AB10;
    /// Computes this frame's movement speed for the character: the magnitude of the animation
    /// control's raw root velocity -- so the speed envelope, including the ramp-up through the
    /// start acts, comes from the animation clips themselves. With `use_blackboard_override` set,
    /// the blackboard float id `1707123197` replaces the result when present. Multiplied onto the
    /// normalized displacement direction by the movement task.
    pub unsafe fn EvaluateCharacterSpeed(
        character: *mut crate::character::character::Character,
        use_blackboard_override: bool,
    ) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                use_blackboard_override: bool,
            ) -> f32 = ::std::mem::transmute(Self::EvaluateCharacterSpeed_ADDRESS);
            f(character, use_blackboard_override)
        }
    }
    pub const QueueStarts_ADDRESS: usize = 0x14081BF00;
    /// Queues the directional run-start (wind-up) acts: measures the XZ angle from the body
    /// forward to the blackboard move direction, writes the residual angle correction, and queues
    /// the matching start act (forward/left/right/180, with stunt variants), choosing the
    /// walk-versus-run flavor by comparing `speed` against the settings' threshold. The start
    /// clips' root velocity ramping from zero is the on-foot acceleration.
    pub unsafe fn QueueStarts(
        character: *mut crate::character::character::Character,
        settings: *const crate::input::locomotion::CharacterMovementSettings,
        speed: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                settings: *const crate::input::locomotion::CharacterMovementSettings,
                speed: f32,
            ) -> bool = ::std::mem::transmute(Self::QueueStarts_ADDRESS);
            f(character, settings, speed)
        }
    }
    pub const QueueStops_ADDRESS: usize = 0x140818C90;
    /// Queues the stop acts for run-mode locomotion; taken by the input tasks when the blackboard
    /// speed is not positive or the stop validation triggers.
    pub unsafe fn QueueStops(
        character: *mut crate::character::character::Character,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
            ) -> bool = ::std::mem::transmute(Self::QueueStops_ADDRESS);
            f(character)
        }
    }
    pub const QueueStopTurns_ADDRESS: usize = 0x140832060;
    /// The aim-mode variant of [`QueueStops`](NStateTask_LocoUtil::QueueStops): stop-and-turn acts
    /// that settle the body toward the aim.
    pub unsafe fn QueueStopTurns(
        character: *mut crate::character::character::Character,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
            ) -> bool = ::std::mem::transmute(Self::QueueStopTurns_ADDRESS);
            f(character)
        }
    }
    pub const GetAimMoveAngle_ADDRESS: usize = 0x140831880;
    /// The XZ angle (degrees) from `move_dir` to the aim reference direction: the vector from the
    /// character to the player's aim-target position (or the camera forward while planting an
    /// explosive), both rotated into the character's local frame when the relevant state flag is
    /// set. Consumed by the aim-relative act dispatchers
    /// ([`QueueAimRelativeMoveActions`](NStateTask_LocoUtil::QueueAimRelativeMoveActions),
    /// [`QueueStopTurns`](NStateTask_LocoUtil::QueueStopTurns), and the on-spot turn queuers) to
    /// select directional acts and on-spot turns. `move_dir` is a by-value `CVector3f` in the
    /// source, passed by pointer under the x64 ABI.
    pub unsafe fn GetAimMoveAngle(
        character: *mut crate::character::character::Character,
        move_dir: *const crate::types::math::Vector3,
    ) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                character: *mut crate::character::character::Character,
                move_dir: *const crate::types::math::Vector3,
            ) -> f32 = ::std::mem::transmute(Self::GetAimMoveAngle_ADDRESS);
            f(character, move_dir)
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
#[repr(C, align(8))]
/// The movement actuator task active during a jump's ascending phase — the airborne analogue of
/// [`NStateTask_MovementLocomotionTask`], on a *separate* code path that does not route through
/// [`EvaluateCharacterOrientation`](NStateTask_LocoUtil::EvaluateCharacterOrientation) or
/// [`GetAimMoveAngle`](NStateTask_LocoUtil::GetAimMoveAngle).
pub struct NStateTask_MovementJumpTask {}
impl NStateTask_MovementJumpTask {
    pub const Update_ADDRESS: usize = 0x140833240;
    /// The per-frame update. Computes a desired facing direction, rate-limits the body toward it,
    /// and writes the character orientation (the release inlines the `SetOrientation` writes). The
    /// desired facing has two sources: when
    /// [`m_AimingWeapon`](character::character::AimState::m_AimingWeapon) is set it faces the weapon
    /// aim target (`CPlayerAimControl`'s stored target world position, which tracks the aim camera);
    /// otherwise it falls back to the current world-forward plus the stick-gated camera-relative steer
    /// from [`UpdateFallSteering`](UpdateFallSteering).
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
impl std::convert::AsRef<NStateTask_MovementJumpTask> for NStateTask_MovementJumpTask {
    fn as_ref(&self) -> &NStateTask_MovementJumpTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementJumpTask> for NStateTask_MovementJumpTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementJumpTask {
        self
    }
}
#[repr(C, align(8))]
/// The movement actuator task: applies this frame's locomotion to the character, computing the
/// body orientation via
/// [`EvaluateCharacterOrientation`](NStateTask_LocoUtil::EvaluateCharacterOrientation) (or the MS
/// variant) and the translation from the animation root motion.
pub struct NStateTask_MovementLocomotionTask {}
impl NStateTask_MovementLocomotionTask {
    pub const Update_ADDRESS: usize = 0x140829E80;
    /// The per-frame update.
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
impl std::convert::AsRef<NStateTask_MovementLocomotionTask>
for NStateTask_MovementLocomotionTask {
    fn as_ref(&self) -> &NStateTask_MovementLocomotionTask {
        self
    }
}
impl std::convert::AsMut<NStateTask_MovementLocomotionTask>
for NStateTask_MovementLocomotionTask {
    fn as_mut(&mut self) -> &mut NStateTask_MovementLocomotionTask {
        self
    }
}
pub const UpdateFallSteering_ADDRESS: usize = 0x1407916F0;
/// Air-steer helper: computes the airborne desired facing and steered velocity for a character from
/// its stick input and the camera input matrix (`GetInputMatrix`). `out_facing` defaults to the
/// character's current world-forward (`-m_WorldMatrixT1` third basis row) and is overwritten with the
/// camera-relative steer direction only under meaningful stick input. `out_velocity_norm` /
/// `out_speed` carry the steered velocity. Called from
/// [`NStateTask_MovementJumpTask::Update`](NStateTask_MovementJumpTask::Update) (jump ascent) and
/// `NAirMovement::UpdateAirPhysics` (fall), so it governs body facing across the whole airborne arc.
/// The return is a leaked `out_facing` pointer the callers discard.
pub unsafe fn UpdateFallSteering(
    character: *mut crate::character::character::Character,
    dt: f32,
    tuning: *const ::std::ffi::c_void,
    processed_velocity: *const crate::types::math::Vector3,
    out_facing: *mut crate::types::math::Vector3,
    out_velocity_norm: *mut crate::types::math::Vector3,
    out_speed: *mut f32,
) -> *mut crate::types::math::Vector3 {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
            dt: f32,
            tuning: *const ::std::ffi::c_void,
            processed_velocity: *const crate::types::math::Vector3,
            out_facing: *mut crate::types::math::Vector3,
            out_velocity_norm: *mut crate::types::math::Vector3,
            out_speed: *mut f32,
        ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
            UpdateFallSteering_ADDRESS,
        );
        f(
            character,
            dt,
            tuning,
            processed_velocity,
            out_facing,
            out_velocity_norm,
            out_speed,
        )
    }
}
