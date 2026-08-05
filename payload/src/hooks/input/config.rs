//! Input hook configuration: on-foot movement settings.

use serde::{Deserialize, Serialize};

/// On-foot movement settings.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct MovementConfig {
    /// Force the aim-relative (strafe) locomotion acts on foot, instead of the third-person run
    /// mode where the directional keys rotate the whole body (nauseating in VR). Implemented as a
    /// scoped shim (see [`crate::hooks::input::locomotion`]): the local player's aim flags are
    /// forced to the aim-relative state only while each locomotion task's update runs, and
    /// restored afterwards, so the aim *system* (reticle, auto-aim, ADS) never sees the forced
    /// state. Two known gaps, in-game verified: the aim-loco acts are combat-stance animations
    /// (arms raised, body bladed -- the pose is baked into the animations, not layered by the aim
    /// system), and the continuous body-yaw-tracks-camera behaviour of real aiming is driven by a
    /// separate aim-gated system this shim does not activate, so the body heading is not steered
    /// (reversed-camera backpedal tank-turns). Kept as the acts half of the eventual solution.
    pub force_fps_movement: bool,
    /// Continuously yaw the body toward the camera on foot -- the heading half of FPS movement.
    /// Implemented by writing the camera's ground-plane forward to the character's target-face-dir
    /// blackboard value and forcing the game's own orientation executor
    /// (`NStateTask_LocoUtil::EvaluateCharacterOrientation`) into its face-dir-tracking mode for
    /// the local player, so the native rate-limited turn code does the rotating in every on-foot
    /// state, holstered included. See `crate::hooks::input::locomotion`.
    pub face_camera: bool,
    /// The tracking turn rate: the maximum yaw step, in degrees per orientation update (one per
    /// frame), passed to the orientation executor while [`face_camera`](Self::face_camera) forces
    /// tracking. Must stay positive; the executor divides by it.
    pub face_camera_turn_step: f32,
    /// The half-angle, in degrees, of the input cone around camera-forward within which the
    /// face-camera pin applies while moving (it always applies while idle). At the default 180
    /// the pin always applies; lower it to hand lateral/backward input back to the native steer
    /// (turn-and-run) instead of [`slide_strafe`](Self::slide_strafe).
    pub face_camera_input_cone_deg: f32,
    /// Make lateral and backward input actually translate the character while the body is pinned
    /// to the camera, instead of fighting the turn animations in place. Two overrides for the
    /// local player: the movement task's displacement direction is redirected along the input move
    /// direction after `NStateTask_LocoUtil::EvaluateCharacterDisplacement` computes it (the task
    /// then scales it by the native speed envelope), and `QueueMoveActions` is replaced to always
    /// queue the plain forward move act so the legs play a clean forward run rather than
    /// half-cancelled turn acts. The legs do not match the movement direction (the game ships no
    /// neutral strafe animations) -- deliberate animationless sliding.
    pub slide_strafe: bool,
    /// The yaw correction, in degrees, applied to the input move direction before it is written as
    /// the displacement direction. The direction is consumed in a frame whose ground axes are
    /// rotated from the blackboard move direction's world frame by an amount that in-game tests
    /// have not yet pinned down (candidates disagreed between runs), so it is a live dial: adjust
    /// until W slides away from the camera and D slides right.
    pub slide_rotation_deg: f32,
    /// Reach the target speed instantly while sliding. The native on-foot speed envelope is the
    /// animation's root velocity, so the run-start clips ramp the character up from zero; this
    /// floors `NStateTask_LocoUtil::EvaluateCharacterSpeed`'s result to the blackboard target
    /// speed while input is held, making the motion uniform from the first frame -- the wind-up
    /// stops affecting the movement, which reads much better from a first-person viewpoint.
    pub slide_instant_speed: bool,
    /// Skip the run-start wind-up acts while sliding: when the input tasks would queue a
    /// directional start act, queue the plain forward move act instead -- guarded by the game's
    /// own `TryAct` pre-flight, with the native starts as the fallback when the animation state
    /// machine refuses it. The legs pop straight into the run cycle with no wind-up lean.
    pub slide_skip_starts: bool,
    /// Suppress the vehicle reversing look-behind animation (`ACT_REVERSE` /
    /// `ACT_REVERSE_MOTORBIKE` into the `S_REVERSE_*` states): the acts are dropped at
    /// `Character::QueueAct` for the local player, so Rico keeps facing forward while reversing --
    /// with a player-driven head, looking behind is the player's job, and the forced body turn is
    /// discomforting.
    pub suppress_reverse_look: bool,
    /// Suppress the head-driven body turn during a jump. The airborne actuator
    /// (`NStateTask_MovementJumpTask::Update`) faces the body at the weapon-aim target while
    /// [`m_AimingWeapon`](jc3gi::character::character::AimState::m_AimingWeapon) is set, and in VR
    /// that target follows the HMD gaze -- so turning your head yaws your body mid-jump with no stick
    /// input. This clears the aim bit around the jump update for the local player while the head is
    /// decoupled (the VR source), routing the jump through its non-aiming fallback (current forward
    /// plus stick-gated steer). Restored immediately after. See `crate::hooks::input::locomotion`.
    pub suppress_air_aim_facing: bool,
    /// Remove the camera-relative look-steer from the parachute's steering for the local player
    /// while the head is decoupled. The parachute steering helper
    /// (`NParachuteMovement::GetParachuteSteeringValues`) mixes the *camera input matrix* (the HMD
    /// in VR) into the parachute's steering values and its yaw/pitch velocity springs through the
    /// look-steer block (`m_LookSteer*`, deadzone + max-yaw + exponential shaping) -- so turning
    /// the head past the deadzone turns the chute with no stick input. This subtracts that
    /// look-steer contribution from the steering outputs and zeroes `look_steer_out` while the
    /// head is decoupled, keeping the stick input and the velocity alignment intact. Inert without
    /// a headpose (flatscreen without the latch). See `crate::hooks::input::parachute`.
    pub suppress_parachute_look_steer: bool,
    /// Suppress Rico's periodic idle fidget for the local player -- the weight-shifts and
    /// look-arounds the game plays while standing still. The idle input task
    /// (`NStateTask_InputLocoIdleTask`) queues `ACT_TO_IDLE_ONE_OFF` on an idle timer to drive the
    /// `S_IDLE` -> `S_IDLE_ONE_OFF` variation; with the head driven by the HMD and the body meant to
    /// hold the player's real pose, that motion reads as the body drifting on its own (issue #33).
    /// The act is dropped at `Character::QueueAct` for the local player, so Rico stays in the base
    /// `S_IDLE`; NPCs keep their fidgets. See `crate::hooks::character`.
    pub suppress_idle_fidget: bool,
    /// Suppress the idle *breathing* for the local player -- the subtle chest/shoulder motion the
    /// base idle clip (`S_IDLE`) plays while standing still, distinct from the periodic
    /// [`suppress_idle_fidget`](Self::suppress_idle_fidget) variations. Unlike the fidget there is no
    /// act to drop; instead the animation-clock advance is held (`dt = 0`) for the local player's
    /// controller while it is in `S_IDLE`, so the pose freezes at its current frame. Movement and
    /// every other state run at normal speed, and NPCs are untouched. See `crate::hooks::animation`.
    /// **Off by default** pending in-headset validation -- a held pose is a bigger visual change than
    /// dropping the fidget, and the freeze wants eyes-on before it ships on.
    pub suppress_idle_breathing: bool,
}
impl MovementConfig {
    pub const fn new() -> Self {
        Self {
            // Off by default: the aim-loco acts it forces are combat-stance animations, which
            // obscures assessing the face-camera heading on its own. Turn it on (with a weapon
            // wielded) for the full directional-legs FPS movement.
            force_fps_movement: false,
            face_camera: true,
            face_camera_turn_step: 10.0,
            face_camera_input_cone_deg: 180.0,
            slide_strafe: true,
            // With the world-to-local transform in place this is only the local frame's forward
            // convention; dial live from the Game tab until W slides away from the camera.
            slide_rotation_deg: 0.0,
            slide_instant_speed: true,
            slide_skip_starts: true,
            suppress_reverse_look: true,
            suppress_air_aim_facing: true,
            suppress_parachute_look_steer: true,
            suppress_idle_fidget: true,
            suppress_idle_breathing: false,
        }
    }
}
