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

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&INPUT_LOCO_MOVE_TASK_UPDATE_BINDER)
        .with_static_binder(&INPUT_LOCO_AIM_RELATIVE_TASK_UPDATE_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_ORIENTATION_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_ORIENTATION_MS_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_DISPLACEMENT_BINDER)
        .with_static_binder(&SETUP_TARGET_DIR_BINDER)
        .with_static_binder(&QUEUE_STARTS_BINDER)
        .with_static_binder(&EVALUATE_CHARACTER_SPEED_BINDER)
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

/// The blackboard id of the target face direction (`CVector3f`): the desired body facing the
/// orientation executor yaws toward in its tracking mode. Written per-state by the game's
/// `SetUpTargetFaceDir` tasks; overwritten here with the camera forward just before the executor
/// reads it, which guarantees ordering against the game's own writers.
const FACE_DIR_BLACKBOARD_ID: u32 = 736_589_998;

/// The blackboard id of the camera-relative world-space move direction (`CVector3f`), written each
/// frame by `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir`. Read here to gate the
/// face-camera pin on the input direction.
const MOVE_DIR_BLACKBOARD_ID: u32 = 2_113_030_792;

/// Move-direction magnitudes below this are treated as no input (idle), where the pin always
/// applies so that turning the camera turns the body.
const MOVE_DIR_IDLE_THRESHOLD: f32 = 0.1;

/// The offset of the character's embedded [`ObjectBlackboard`] (`lea rcx, [character+2060h]` at
/// every blackboard call site in the loco tasks). A payload-side constant because pyxis cannot yet
/// embed an opaque unsized field at a fixed offset.
const CHARACTER_BLACKBOARD_OFFSET: usize = 0x2060;

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
/// follows the head once past the latch threshold), and while the headpose drives the view on
/// foot, the idle pin is suppressed so the head can decouple from the body within the latch cone.
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

    // If the sim's latch is active, the body should follow the head. This applies regardless of
    // the face_camera toggle (reusing face_camera_turn_step as the turn rate) and intentionally
    // bypasses the move-dir cone check: when the latch is active, body-follow takes priority over
    // strafe.
    if let Some(face_dir) = crate::headpose::sim::body_yaw_target() {
        unsafe {
            let value = Vector3 {
                data: [face_dir.x, face_dir.y, face_dir.z],
            };
            (*blackboard).SetVector3(FACE_DIR_BLACKBOARD_ID, &value, 1, std::ptr::null());
        }
        FACE_CAMERA_CALLS.fetch_add(1, Ordering::Relaxed);
        return (true, step.max(0.1));
    }

    if !face_camera {
        return passthrough;
    }
    let Some(face_dir) = camera_ground_forward() else {
        return passthrough;
    };

    match read_move_dir(blackboard) {
        // Idle with the headpose driving the view: the camera forward *is* the head forward (the
        // context transform the input matrix reads is patched from the headpose), so the idle pin
        // would make the body track the head at any offset and the decoupled cone could never
        // open. The latch above owns idle body turning instead.
        None if crate::headpose::is_active()
            && crate::headpose::sim::mode() == crate::headpose::sim::HeadMode::OnFoot =>
        {
            return passthrough;
        }
        // Idle without the headpose: pin, so turning the camera turns the body.
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
        (*blackboard).SetVector3(FACE_DIR_BLACKBOARD_ID, &value, 1, std::ptr::null());
    }
    FACE_CAMERA_CALLS.fetch_add(1, Ordering::Relaxed);
    (true, step.max(0.1))
}

fn character_blackboard(character: &mut Character) -> *mut ObjectBlackboard {
    (character as *mut Character as *mut u8)
        .wrapping_add(CHARACTER_BLACKBOARD_OFFSET)
        .cast()
}

/// The current move direction on the ground plane, or `None` while idle (no meaningful input) or
/// when the blackboard value is absent -- both of which the pin treats as "apply".
fn read_move_dir(blackboard: *mut ObjectBlackboard) -> Option<glam::Vec3> {
    let mut value = Vector3::default();
    unsafe {
        if !(*blackboard).GetVector3(MOVE_DIR_BLACKBOARD_ID, &mut value) {
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
                (*blackboard).SetVector3(MOVE_DIR_BLACKBOARD_ID, &value, 1, std::ptr::null());
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
    let Some(target) = read_float(character_blackboard(character), SPEED_BLACKBOARD_ID) else {
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
            if (*blackboard).GetVector3(CONSTRAINED_DIR_BLACKBOARD_ID, &mut existing) {
                (*blackboard).SetVector3(
                    CONSTRAINED_DIR_BLACKBOARD_ID,
                    &world_value,
                    1,
                    std::ptr::null(),
                );
            }
        }

        SLIDE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

/// The blackboard id of the constrained-ground movement direction, present on slopes (and other
/// surface-constrained locomotion): when set, the movement task blends its displacement direction
/// toward it instead of using the primary displacement output.
const CONSTRAINED_DIR_BLACKBOARD_ID: u32 = 2_485_695_409;

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
    let move_dir = unsafe { (*blackboard).GetVector3(MOVE_DIR_BLACKBOARD_ID, &mut move_dir) }
        .then(|| glam::Vec3::new(move_dir.data[0], move_dir.data[1], move_dir.data[2]));
    *SNAPSHOT.lock() = Some(BlackboardSnapshot {
        input_magnitude: move_input_magnitude(),
        move_dir,
        speed: read_float(blackboard, SPEED_BLACKBOARD_ID),
        aux_float: read_float(blackboard, AUX_FLOAT_BLACKBOARD_ID),
    });
}

/// The blackboard id of the float speed value the input locomotion tasks branch on (`<= 0` routes
/// into the stop acts).
const SPEED_BLACKBOARD_ID: u32 = 3_396_837_917;

/// The second float the input move task reads alongside the speed; semantics unmapped, shown in
/// the readout as a candidate input-strength signal.
const AUX_FLOAT_BLACKBOARD_ID: u32 = 2_217_900_102;
