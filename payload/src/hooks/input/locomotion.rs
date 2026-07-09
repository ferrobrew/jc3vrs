//! `NStateTask_InputLoco*` detours: the scoped aim-state shim behind
//! [`force_fps_movement`](crate::config::MovementConfig::force_fps_movement).
//!
//! The on-foot locomotion tasks choose between run/steer (the body rotates toward the movement
//! direction) and aim-relative strafe (the body faces the aim reference, the directional keys
//! strafe) by reading [`Character::m_AimFlags`] -- the same flags that drive the weapon raise, the
//! reticle, and auto-aim. Forcing the flags globally would drag all of that in, so the shim forces
//! them only for the duration of each locomotion task's `Update` and restores them afterwards: the
//! movement branch always sees "aiming", the aim system never does.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::{
    blackboard::ObjectBlackboard,
    camera::game_camera_manager::GameCameraManager,
    character::character::{AimState, Character},
    input::{
        input_action_map::{Action, LocalPlayerActionMap},
        locomotion::{CharacterMovementSettings, get_NCharacter_ActMoveNoAim},
    },
    state::StateContext,
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    headpose::sim::{HeadMode, LatchState},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&INPUT_LOCO_MOVE_TASK_UPDATE_BINDER)
        .with_static_binder(&INPUT_LOCO_AIM_RELATIVE_TASK_UPDATE_BINDER)
        .with_static_binder(&MOVEMENT_JUMP_TASK_UPDATE_BINDER)
        .with_static_binder(&UPDATE_FALL_STEERING_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_ORIENTATION_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_ORIENTATION_MS_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_DISPLACEMENT_BINDER)
        .with_static_binder(&SETUP_TARGET_DIR_BINDER)
        .with_static_binder(&QUEUE_STARTS_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_SPEED_BINDER)
        .with_static_binder(&GET_AIM_MOVE_ANGLE_BINDER)
}

/// Diagnostic counters for the Game tab, so hook liveness and the shim's activity are visible
/// in-game: how often each shimmed task ran, how often the shim actually forced the aim state,
/// and how often the orientation executor was forced into face-dir tracking.
pub static MOVE_TASK_CALLS: AtomicU64 = AtomicU64::new(0);
pub static AIM_RELATIVE_TASK_CALLS: AtomicU64 = AtomicU64::new(0);
/// How often the orientation evaluator ran for the local player. Unlike the task counters above,
/// this advances every on-foot frame including idle (the move/aim tasks stop running while idle),
/// which is what makes it usable as the headpose sim's on-foot signal.
pub static ORIENTATION_EVAL_CALLS: AtomicU64 = AtomicU64::new(0);
pub static SHIMMED_CALLS: AtomicU64 = AtomicU64::new(0);
pub static FACE_CAMERA_CALLS: AtomicU64 = AtomicU64::new(0);
pub static SLIDE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static SKIPPED_STARTS: AtomicU64 = AtomicU64::new(0);
pub static INSTANT_SPEED_FLOORS: AtomicU64 = AtomicU64::new(0);

#[detour(
    address = jc3gi::input::locomotion::NStateTask_InputLocoMoveTask::Update_ADDRESS
)]
fn input_loco_move_task_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    MOVE_TASK_CALLS.fetch_add(1, Ordering::Relaxed);
    with_forced_aim_state(ctx, || {
        INPUT_LOCO_MOVE_TASK_UPDATE.get().unwrap().call(ctx, p1, p2);
    });
}

#[detour(
    address = jc3gi::input::locomotion::NStateTask_InputLocoAimRelativeTask::Update_ADDRESS
)]
fn input_loco_aim_relative_task_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    AIM_RELATIVE_TASK_CALLS.fetch_add(1, Ordering::Relaxed);
    with_forced_aim_state(ctx, || {
        INPUT_LOCO_AIM_RELATIVE_TASK_UPDATE
            .get()
            .unwrap()
            .call(ctx, p1, p2);
    });
}

/// The airborne (jump) actuator. Its aim-target-facing branch turns the body toward the weapon-aim
/// target while [`AimState::m_AimingWeapon`] is set, and in VR that target follows the HMD gaze, so a
/// head turn yaws the body mid-jump with no stick input. While the head is decoupled (the VR source),
/// clear the aim bit around the update for the local player so the jump takes its non-aiming fallback
/// (current forward plus the stick-gated steer) -- the mirror of [`with_forced_aim_state`], clearing
/// rather than forcing. Restored immediately; the character updates on its own worker thread, so the
/// temporary clear is invisible to the aim system. Passes straight through with the toggle off, off
/// the VR source, or for a non-local character.
#[detour(
    address = jc3gi::input::locomotion::NStateTask_MovementJumpTask::Update_ADDRESS
)]
fn movement_jump_task_update(ctx: *mut StateContext, p1: *mut c_void, p2: *mut c_void) {
    let character = if Config::lock_query(|c| c.movement.suppress_air_aim_facing) {
        unsafe { character_from_context(ctx) }.filter(|c| c.m_IsLocalCharacter && head_decoupled(c))
    } else {
        None
    };
    // Clear the weapon-aim bit around the update so its aim-target-facing branch takes the
    // non-aiming fallback (mirror of `with_forced_aim_state`, clearing rather than forcing). This
    // covers only the aim-target facing path; the stick-gated air-steer is handled separately in
    // `update_fall_steering`.
    match character {
        Some(character) => {
            let saved = character.m_AimFlags;
            let mut cleared = saved;
            cleared.remove(AimState::m_AimingWeapon);
            character.m_AimFlags = cleared;
            MOVEMENT_JUMP_TASK_UPDATE.get().unwrap().call(ctx, p1, p2);
            character.m_AimFlags = saved;
        }
        None => MOVEMENT_JUMP_TASK_UPDATE.get().unwrap().call(ctx, p1, p2),
    }
}

/// Hold the local player's body facing straight while airborne under VR. `UpdateFallSteering`
/// overwrites `out_facing` with a camera-relative steer direction under meaningful stick input, and
/// in VR the camera follows the HMD, so pushing to move while turning the head yaws the body
/// mid-jump. After the original runs, restore `out_facing` to the character's current world-forward
/// (`-m_WorldMatrixT1` third basis row -- the same value the function uses as its no-steer default)
/// for the local player while the head is decoupled, leaving the steered velocity
/// (`out_velocity_norm` / `out_speed`) untouched so the stick still moves the body without turning
/// it. Governs the whole airborne arc: the jump ascent calls this directly, the fall via
/// `NAirMovement::UpdateAirPhysics`. NPCs and the flatscreen / non-decoupled player pass through.
#[detour(address = jc3gi::input::locomotion::UpdateFallSteering_ADDRESS)]
fn update_fall_steering(
    character: *mut Character,
    dt: f32,
    tuning: *const c_void,
    processed_velocity: *const Vector3,
    out_facing: *mut Vector3,
    out_velocity_norm: *mut Vector3,
    out_speed: *mut f32,
) -> *mut Vector3 {
    let result = UPDATE_FALL_STEERING.get().unwrap().call(
        character,
        dt,
        tuning,
        processed_velocity,
        out_facing,
        out_velocity_norm,
        out_speed,
    );
    let hold_straight = Config::lock_query(|c| c.movement.suppress_air_aim_facing)
        && unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter && head_decoupled(c));
    if hold_straight && !out_facing.is_null() {
        let forward = -glam::Mat4::from(unsafe { (*character).m_WorldMatrixT1 })
            .z_axis
            .truncate();
        unsafe {
            (*out_facing).data = [forward.x, forward.y, forward.z];
        }
    }
    result
}

/// Run `call` (the wrapped task update) with the local player's aim flags forced to the
/// aim-relative state, restoring them afterwards. Passes straight through when the toggle is off,
/// the context has no character, or the character is not the local player. The restore makes the
/// force invisible to everything outside the wrapped call; the character updates on its own worker
/// thread, so nothing else reads the flags mid-task.
fn with_forced_aim_state(ctx: *mut StateContext, call: impl FnOnce()) {
    let character = if Config::lock_query(|c| c.movement.force_fps_movement) {
        unsafe { character_from_context(ctx).filter(|c| c.m_IsLocalCharacter) }
    } else {
        None
    };
    let Some(character) = character else {
        call();
        return;
    };

    let saved = character.m_AimFlags;
    let mut forced = saved | AimState::m_AimingWeapon;
    // Mimic the game's own "was aiming last frame" latch: on the first shimmed frame after a gap
    // (loco tasks stopped running -- idle, vehicle, toggle off), leave `m_WasAiming` clear so the
    // task queues the one-shot transition act that moves the animation state machine into the
    // aim-relative family; on contiguous frames set it so the steady acts are queued instead. The
    // steady acts alone are dropped by the rule system unless that transition has run, which is
    // what made the strafe engage only sporadically. When really aiming, the saved flags already
    // carry the game's own latch and are left alone.
    let now = Instant::now();
    let contiguous = LAST_SHIM_AT
        .lock()
        .replace(now)
        .is_some_and(|last| now.duration_since(last) < SHIM_CONTINUITY);
    if !saved.intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple) && contiguous {
        forced |= AimState::m_WasAiming;
    }

    character.m_AimFlags = forced;
    SHIMMED_CALLS.fetch_add(1, Ordering::Relaxed);
    call();
    character.m_AimFlags = saved;
}

/// The longest gap between shimmed task updates still treated as one contiguous run of forcing;
/// anything longer re-queues the transition act. Comfortably above one frame at any playable frame
/// rate, and short enough that a real interruption (vehicle, idle) restarts the transition.
const SHIM_CONTINUITY: Duration = Duration::from_millis(250);

/// When the shim last forced the local player's flags; `None` until the first shimmed call.
static LAST_SHIM_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Recover the [`Character`] from a state-task context. The context's first pointer addresses a
/// sub-object at character + 8; the game type-checks that sub-object against `CCharacter::TYPE_ID`
/// through its vtable and then subtracts (see `NStateTask_InputLocoMoveTask::Update` in the IDB).
/// The RTTI check is not replicated here: the hooked tasks only run on character state machines.
unsafe fn character_from_context(ctx: *mut StateContext) -> Option<&'static mut Character> {
    unsafe {
        let sub_object = *(ctx as *const *mut u8);
        if sub_object.is_null() {
            return None;
        }
        (sub_object.sub(8) as *mut Character).as_mut()
    }
}

#[detour(
    address = jc3gi::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterOrientation_ADDRESS
)]
fn evaluate_character_orientation(
    out: *mut Matrix4,
    character: *mut Character,
    track_face_dir: bool,
    snap_to_face_dir: bool,
    wait_for_blend: bool,
    max_step_deg: f32,
    dt: f32,
) -> *mut Matrix4 {
    let (track_face_dir, max_step_deg) = force_face_camera(character, track_face_dir, max_step_deg);
    EVALUATE_CHARACTER_ORIENTATION.get().unwrap().call(
        out,
        character,
        track_face_dir,
        snap_to_face_dir,
        wait_for_blend,
        max_step_deg,
        dt,
    )
}

#[detour(
    address = jc3gi::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterOrientationMS_ADDRESS
)]
fn evaluate_character_orientation_ms(
    out: *mut Matrix4,
    character: *mut Character,
    track_face_dir: bool,
    snap_to_face_dir: bool,
    max_step_deg: f32,
    dt: f32,
) -> *mut Matrix4 {
    let (track_face_dir, max_step_deg) = force_face_camera(character, track_face_dir, max_step_deg);
    EVALUATE_CHARACTER_ORIENTATION_MS.get().unwrap().call(
        out,
        character,
        track_face_dir,
        snap_to_face_dir,
        max_step_deg,
        dt,
    )
}

/// Move-direction magnitudes below this are treated as no input (idle), where the pin always
/// applies so that turning the camera turns the body.
const MOVE_DIR_IDLE_THRESHOLD: f32 = 0.1;

/// The heading half of FPS movement: for the local player, write the camera's ground-plane forward
/// to the target-face-dir blackboard value and force the orientation executor's tracking mode with
/// the configured turn step. Everything else (NPCs, toggle off, no camera) passes through
/// untouched. The step is clamped positive because the executor divides by it.
///
/// The pin is gated on the input direction: it always applies while idle (turning the camera turns
/// the body), and while moving it only applies within the configured cone around camera-forward.
/// Outside the cone the native steer runs instead -- the run animations only carry forward root
/// motion, so pinning the yaw against lateral or backward input leaves the character fighting its
/// own turn acts in place rather than strafing.
///
/// The headpose sim layers on top: its latch target takes priority over the camera pin (the body
/// follows the head once past the latch threshold), and while the head is decoupled on foot (idle,
/// not really aiming), the executor is forced *out* of face-dir tracking entirely — the
/// aim-relative family otherwise tracks the game's own face dir, which derives from the aim
/// reference (the head), turning the body with the head regardless of the pin.
fn force_face_camera(
    character: *mut Character,
    track_face_dir: bool,
    max_step_deg: f32,
) -> (bool, f32) {
    let passthrough = (track_face_dir, max_step_deg);
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return passthrough;
    };
    // The counter and snapshot run before any config gating: the headpose sim's mode detection and
    // the Game tab readout must see every on-foot frame regardless of the toggles below.
    ORIENTATION_EVAL_CALLS.fetch_add(1, Ordering::Relaxed);
    capture_snapshot(character);

    let blackboard = character_blackboard(character);
    let (face_camera, step, cone_deg) = Config::lock_query(|c| {
        (
            c.movement.face_camera,
            c.movement.face_camera_turn_step,
            c.movement.face_camera_input_cone_deg,
        )
    });

    // Steer the body toward the headpose's requested heading, if any: the flatscreen sim's
    // head-follow latch target, or the VR source's look-driven body yaw. This applies regardless of
    // the face_camera toggle (reusing face_camera_turn_step as the turn rate) and intentionally
    // bypasses the move-dir cone check: the requested heading takes priority over strafe, so the
    // native turn-toward-movement never fights it (which is what let backpedaling tank-turn the body
    // in VR).
    if let Some(face_dir) = crate::headpose::body_yaw_target() {
        unsafe {
            let value = Vector3 {
                data: [face_dir.x, face_dir.y, face_dir.z],
            };
            (*blackboard).SetVector3(
                ObjectBlackboard::TARGET_FACE_DIR_ID,
                &value,
                1,
                std::ptr::null(),
            );
        }
        FACE_CAMERA_CALLS.fetch_add(1, Ordering::Relaxed);
        return (true, step.max(0.1));
    }

    // While the head is decoupled, force the executor *out* of face-dir tracking rather than
    // passing through: in the aim-relative family the game itself passes track_face_dir with its
    // own blackboard face dir, which derives from the aim reference — the head — so a passthrough
    // still yaws the body with the head continuously. The act-level suppression in
    // [`get_aim_move_angle`] stops the turn *acts*; this stops the executor's continuous yaw.
    // Applies regardless of the face_camera toggle, like the latch.
    if head_decoupled(character) {
        return (false, max_step_deg);
    }

    if !face_camera {
        return passthrough;
    }
    let Some(face_dir) = camera_ground_forward() else {
        return passthrough;
    };

    match read_move_dir(blackboard) {
        // Idle: pin, so turning the camera turns the body. (With the headpose active, idle
        // reaches here only when really aiming — the decoupled-idle case returned above — and the
        // camera forward is the head forward, which is exactly the aim-tracking behavior.)
        None => {}
        // Moving: pin only within the configured cone around camera-forward (outside it, the
        // native steer runs; see the function docs).
        Some(move_dir) if move_dir.angle_between(face_dir).to_degrees() > cone_deg => {
            return passthrough;
        }
        Some(_) => {}
    }

    unsafe {
        let value = Vector3 {
            data: [face_dir.x, face_dir.y, face_dir.z],
        };
        (*blackboard).SetVector3(
            ObjectBlackboard::TARGET_FACE_DIR_ID,
            &value,
            1,
            std::ptr::null(),
        );
    }
    FACE_CAMERA_CALLS.fetch_add(1, Ordering::Relaxed);
    (true, step.max(0.1))
}

/// Whether the body must not chase the head this frame — so both the orientation executor's face-dir
/// tracking and the aim-relative turn acts are suppressed. On foot with the headpose active.
///
/// Under the **VR source** this is *always* true on foot: the HMD owns the head and the stick owns
/// the body, so the body must never turn toward the head — moving, aiming, or idle. The
/// body-relative pose composition (`body × cockpit`) makes any body-follow of the head a runaway
/// feedback loop; idle it spins in place, and walking it spins in circles. (The sim's latch state is
/// not even updated while VR publishes, so the flatscreen conditions below cannot be consulted.)
///
/// Under the **flatscreen sim** it is the decoupled-idle-not-aiming state: the latch owns body
/// turning past its threshold, and while idle within the cone the head moves freely, so the body is
/// held only in that specific window.
fn head_decoupled(character: &Character) -> bool {
    if !crate::headpose::is_active() {
        return false;
    }
    // Under the VR source the HMD owns the head and the stick owns the body in *every* mode -- on
    // foot, airborne (a jump), hanging from a tether under a helicopter, or in a vehicle -- so the
    // body must never turn toward the head. Gating this on `OnFoot` (as the flatscreen branch below
    // must, since there the head *is* the mouse-look that should turn the body) let the native
    // aim-turn drag the body around whenever the mode detector fell out of `OnFoot`: the yaw kick on
    // jumping and the head-driven body spin while dangling from a helicopter both land in `Other`,
    // where the suppression used to switch off.
    if crate::headpose::source() == crate::headpose::Source::Vr {
        return true;
    }
    if crate::headpose::sim::mode() != HeadMode::OnFoot {
        return false;
    }
    crate::headpose::sim::latch_state() == LatchState::Decoupled
        && !character
            .m_AimFlags
            .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple)
        && !move_input_magnitude().is_some_and(|magnitude| magnitude >= INPUT_DEADZONE)
}

fn character_blackboard(character: &mut Character) -> *mut ObjectBlackboard {
    &raw mut character.m_Blackboard
}

/// The current move direction on the ground plane, or `None` while idle (no meaningful input) or
/// when the blackboard value is absent -- both of which the pin treats as "apply".
fn read_move_dir(blackboard: *mut ObjectBlackboard) -> Option<glam::Vec3> {
    let mut value = Vector3::default();
    unsafe {
        if !(*blackboard).GetVector3(ObjectBlackboard::MOVE_DIR_ID, &mut value) {
            return None;
        }
    }
    let flat = glam::Vec3::new(value.data[0], 0.0, value.data[2]);
    (flat.length_squared() > MOVE_DIR_IDLE_THRESHOLD * MOVE_DIR_IDLE_THRESHOLD)
        .then(|| flat.normalize())
}

/// The camera's forward on the ground plane, from the same input matrix the game uses to make the
/// move direction camera-relative (its negated third row is camera-forward; see
/// `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir`). `None` when the camera manager is
/// absent or the camera looks straight up or down.
fn camera_ground_forward() -> Option<glam::Vec3> {
    unsafe {
        let camera_manager = GameCameraManager::get()?;
        let mut matrix = Matrix4::default();
        camera_manager.GetInputMatrix(&mut matrix);
        let forward = -glam::Mat4::from(matrix).z_axis.truncate();
        let flat = glam::Vec3::new(forward.x, 0.0, forward.z);
        (flat.length_squared() > 1e-4).then(|| flat.normalize())
    }
}

/// The local player's current move direction for the slide override: the *real* input direction
/// captured by the [`setup_target_dir`] detour before it spoofs the blackboard. `None` when the
/// slide is inactive (toggle off, no input, really aiming, or not on foot this frame).
fn slide_move_dir(character: *mut Character) -> Option<glam::Vec3> {
    if !(unsafe { character.as_ref() }).is_some_and(|c| c.m_IsLocalCharacter) {
        return None;
    }
    *REAL_MOVE_DIR.lock()
}

/// The real (pre-spoof) camera-relative move direction for the local player this frame, captured
/// by [`setup_target_dir`]; `None` whenever the slide is inactive.
static REAL_MOVE_DIR: Mutex<Option<glam::Vec3>> = Mutex::new(None);

/// The magnitude of the current movement input, read from the local player's action-map effectors
/// (the same slots `SetupTargetDir` reads). The move-dir blackboard cannot supply this --
/// `SetupTargetDir` re-writes the previous direction below the deadzone, so the blackboard
/// direction is always a unit vector and carries no held/released information.
fn move_input_magnitude() -> Option<f32> {
    unsafe {
        let map = LocalPlayerActionMap::get()?;
        let mut value = |action: Action| {
            map.GetActionEffector(action)
                .as_ref()
                .map_or(0.0, |effector| effector.m_Value)
        };
        let x = value(Action::MOVE_RIGHT) - value(Action::MOVE_LEFT);
        let y = value(Action::MOVE_FORWARD) - value(Action::MOVE_BACKWARD);
        Some(glam::Vec2::new(x, y).length())
    }
}

/// Movement-input magnitudes below this are treated as no input.
const INPUT_DEADZONE: f32 = 0.15;

/// The source-level fix for the animation war the slide used to fight downstream: while sliding,
/// overwrite the freshly written blackboard move direction with camera-forward, so every consumer
/// in the input-task layer -- act selection, starts, stops, plant-and-turns, rotate-on-spot --
/// natively behaves as if the player were moving forward: calm forward locomotion, no turn acts to
/// cancel, no start resistance. The *real* input direction is captured first and consumed only by
/// the displacement override, which is where the actual movement direction is decided. Skipped
/// while really aiming, so the native aim-strafe (which needs the true direction for its
/// directional legs) is untouched.
#[detour(
    address = jc3gi::input::locomotion::NStateTask_InputLocoSetTargetDirTask::SetupTargetDir_ADDRESS
)]
fn setup_target_dir(character: *mut Character, target_dir: *mut c_void, props: *mut c_void) -> f64 {
    let result = SETUP_TARGET_DIR
        .get()
        .unwrap()
        .call(character, target_dir, props);

    // `SetupTargetDir`'s `this` is the character (see the pyxis def); it only runs for the local
    // player's on-foot input states.
    let eligible = Config::lock_query(|c| c.movement.slide_strafe)
        && (unsafe { character.as_ref() }).is_some_and(|c| {
            c.m_IsLocalCharacter
                && !c
                    .m_AimFlags
                    .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple)
        })
        && move_input_magnitude().is_some_and(|magnitude| magnitude >= INPUT_DEADZONE);

    let mut real_dir = None;
    if eligible && let Some(character) = unsafe { character.as_mut() } {
        let blackboard = character_blackboard(character);
        real_dir = read_move_dir(blackboard);
        if let Some(spoof) = camera_ground_forward() {
            let value = Vector3 {
                data: [spoof.x, 0.0, spoof.z],
            };
            unsafe {
                (*blackboard).SetVector3(
                    ObjectBlackboard::MOVE_DIR_ID,
                    &value,
                    1,
                    std::ptr::null(),
                );
            }
        }
    }
    *REAL_MOVE_DIR.lock() = real_dir;

    result
}

/// While sliding, replace the directional run-start (wind-up) acts with the plain forward move
/// act, so the legs pop straight into the run cycle. The start clips' lean reads poorly from a
/// first-person viewpoint, and their ramping root velocity is the on-foot acceleration (see
/// [`evaluate_character_speed`]). Guarded by the game's own act pre-flight
/// (`AnimatedModel::TryAct`), exactly as the game's dispatchers guard their queues: when the
/// animation state machine will not accept the move act from the current state, the native starts
/// run instead.
#[detour(address = jc3gi::input::locomotion::NStateTask_LocoUtil::QueueStarts_ADDRESS)]
fn queue_starts(
    character: *mut Character,
    settings: *const CharacterMovementSettings,
    speed: f32,
) -> bool {
    if Config::lock_query(|c| c.movement.slide_skip_starts)
        && slide_move_dir(character).is_some()
        && let Some(character) = unsafe { character.as_mut() }
    {
        let act: *const u32 = unsafe { get_NCharacter_ActMoveNoAim() };
        if unsafe { character.m_AnimatedModel.TryAct(act) } {
            unsafe { character.QueueAct(act) };
            SKIPPED_STARTS.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }
    QUEUE_STARTS.get().unwrap().call(character, settings, speed)
}

/// While the headpose decouples the head from the body on foot (idle, unlatched, and not really
/// aiming), report the aim-move angle as zero. The aim-relative act dispatchers otherwise measure
/// the angle to the player's aim target — which follows the head through the `GetCameraMatrix`
/// override — and queue rotate-on-spot acts that turn the body toward the head long before the
/// latch engages. Zero reads as "already aligned", so no turn acts are queued and the body stays
/// put. When latched, really aiming, or moving, the original runs: the angle then points at the
/// head, and the game's own turn machinery performs the latch catch-up natively.
#[detour(address = jc3gi::input::locomotion::NStateTask_LocoUtil::GetAimMoveAngle_ADDRESS)]
fn get_aim_move_angle(character: *mut Character, move_dir: *const Vector3) -> f32 {
    if (unsafe { character.as_ref() }).is_some_and(|c| c.m_IsLocalCharacter && head_decoupled(c)) {
        return 0.0;
    }
    GET_AIM_MOVE_ANGLE.get().unwrap().call(character, move_dir)
}

/// While sliding, floor the movement speed to the blackboard target speed. The native speed
/// envelope is the magnitude of the animation's root velocity, so the start clips ramp the
/// character up from zero across the wind-up; flooring to the target makes the motion uniform
/// from the first frame. The slide engagement drops as soon as the input is released, so stops
/// keep the native decaying envelope.
#[detour(
    address = jc3gi::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterSpeed_ADDRESS
)]
fn evaluate_character_speed(character: *mut Character, use_blackboard_override: bool) -> f32 {
    let speed = EVALUATE_CHARACTER_SPEED
        .get()
        .unwrap()
        .call(character, use_blackboard_override);
    if !Config::lock_query(|c| c.movement.slide_instant_speed)
        || slide_move_dir(character).is_none()
    {
        return speed;
    }
    let Some(character) = (unsafe { character.as_mut() }) else {
        return speed;
    };
    let Some(target) = read_float(character_blackboard(character), ObjectBlackboard::SPEED_ID)
    else {
        return speed;
    };
    if target > speed {
        INSTANT_SPEED_FLOORS.fetch_add(1, Ordering::Relaxed);
        target
    } else {
        speed
    }
}

// The parameter list is the game function's ABI, not a designable API, so the parameter-struct
// convention cannot apply -- the game calls this signature.
#[expect(clippy::too_many_arguments)]
#[detour(
    address = jc3gi::input::locomotion::NStateTask_LocoUtil::EvaluateCharacterDisplacement_ADDRESS
)]
fn evaluate_character_displacement(
    character: *mut Character,
    orientation: *const Matrix4,
    p3: bool,
    p4: bool,
    p5: bool,
    p6: f32,
    out_secondary: *mut Vector3,
    out_direction: *mut Vector3,
) {
    EVALUATE_CHARACTER_DISPLACEMENT.get().unwrap().call(
        character,
        orientation,
        p3,
        p4,
        p5,
        p6,
        out_secondary,
        out_direction,
    );

    // Redirect the movement along the input direction: the movement task normalizes this vector
    // and scales it by the native speed, so replacing the direction slides the character where the
    // input points while the legs play whatever the animation state machine chose. The animation
    // root motion's slope-following Y is dropped; the physics step reconciles the ground contact.
    //
    // The direction is consumed in the *character's* frame, not world space (established in-game:
    // a world-space write slid in facing-dependent directions, correct at exactly one body yaw),
    // so bring the world move direction into the frame of the orientation matrix the game passed
    // in. The remaining fixed offset -- the local frame's own forward convention -- is the
    // [`slide_rotation_deg`](crate::config::MovementConfig::slide_rotation_deg) dial, applied in
    // the local frame. If the slide still tracks the facing after this (error doubling with yaw
    // instead of cancelling), the rotation convention of `Matrix4` -> glam is inverted here and
    // the `inverse()` should be dropped.
    if let Some(move_dir) = slide_move_dir(character)
        && let Some(out_direction) = unsafe { out_direction.as_mut() }
        && let Some(character) = unsafe { character.as_mut() }
    {
        // The yaw dial (the local frame's forward convention) applied in world space; equivalent
        // to the previous local-space application for pure-yaw orientations, and the world vector
        // is needed anyway for the slope path below.
        let theta = Config::lock_query(|c| c.movement.slide_rotation_deg).to_radians();
        let (sin, cos) = theta.sin_cos();
        let world = glam::Vec3::new(
            move_dir.x * cos + move_dir.z * sin,
            0.0,
            -move_dir.x * sin + move_dir.z * cos,
        );

        let (_, rotation, _) =
            unsafe { glam::Mat4::from(*orientation) }.to_scale_rotation_translation();
        let local = rotation.inverse() * world;
        out_direction.data = [local.x, 0.0, local.z];

        // The movement task discards `out_direction` when the constrained-ground blackboard value
        // is present (slopes): it blends `out_secondary` toward that value and rotates the result
        // into the local frame instead -- and the slope task derives the value from the *spoofed*
        // move dir, so on slopes every input walked camera-forward. Override both carriers with
        // the real world direction: the secondary output (the blend base) and, only when already
        // present, the blackboard value itself (writing it unconditionally would activate the
        // slope path on flat ground).
        let world_value = Vector3 {
            data: [world.x, 0.0, world.z],
        };
        if let Some(out_secondary) = unsafe { out_secondary.as_mut() } {
            *out_secondary = world_value;
        }
        let blackboard = character_blackboard(character);
        let mut existing = Vector3::default();
        unsafe {
            if (*blackboard).GetVector3(ObjectBlackboard::CONSTRAINED_DIR_ID, &mut existing) {
                (*blackboard).SetVector3(
                    ObjectBlackboard::CONSTRAINED_DIR_ID,
                    &world_value,
                    1,
                    std::ptr::null(),
                );
            }
        }

        SLIDE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

fn read_float(blackboard: *mut ObjectBlackboard, id: u32) -> Option<f32> {
    let mut value = 0.0f32;
    unsafe { (*blackboard).GetFloat(id, &mut value) }.then_some(value)
}

/// A diagnostic snapshot of the local player's locomotion state, captured on the game thread (the
/// blackboard accessors return nothing when called from the render/UI thread) and stashed for the
/// Game tab. Every field is independently optional so a single absent value cannot hide the line.
/// The speed value is the input tasks' branch signal (`<= 0` routes into the stop acts) and the
/// prime suspect for backpedal misbehaviour.
#[derive(Clone, Copy, Default)]
pub struct BlackboardSnapshot {
    pub input_magnitude: Option<f32>,
    pub move_dir: Option<glam::Vec3>,
    pub speed: Option<f32>,
    pub aux_float: Option<f32>,
}

/// The last game-thread-captured snapshot, for the UI. `None` until the first capture.
static SNAPSHOT: Mutex<Option<BlackboardSnapshot>> = Mutex::new(None);

pub fn debug_blackboard_snapshot() -> Option<BlackboardSnapshot> {
    *SNAPSHOT.lock()
}

/// Capture the snapshot; called from the orientation-evaluator detour, which runs on the game
/// thread every frame the local player is on foot.
fn capture_snapshot(character: &mut Character) {
    let blackboard = character_blackboard(character);
    let mut move_dir = Vector3::default();
    let move_dir =
        unsafe { (*blackboard).GetVector3(ObjectBlackboard::MOVE_DIR_ID, &mut move_dir) }
            .then(|| glam::Vec3::new(move_dir.data[0], move_dir.data[1], move_dir.data[2]));
    *SNAPSHOT.lock() = Some(BlackboardSnapshot {
        input_magnitude: move_input_magnitude(),
        move_dir,
        speed: read_float(blackboard, ObjectBlackboard::SPEED_ID),
        aux_float: read_float(blackboard, ObjectBlackboard::AUX_FLOAT_ID),
    });
}
