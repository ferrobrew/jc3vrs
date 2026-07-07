use std::{
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::{
    animation::{
        ik::{HumanIK, Pass, SolveStep},
        symbol_table::EventIdSymbolTable,
    },
    character::character::{AnimatedModel, Character, Joint, SafeBoneIndex},
    hash::hashlittle,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, headpose};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL_BINDER)
        .with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
        .with_static_binder(&CHARACTER_QUEUE_ACT_BINDER)
}

/// The tracing target for the body-IK path.
const BODY_IK_TARGET: &str = "body_ik";

/// The global HumanIK enable flag (`CCharacter::m_EnableHIK`, `byte_142D621C8`): the engine gates
/// the whole IK pass on it in `UpdatePassFinalizePose_Parallel`. No pyxis binding exists for this
/// standalone global, so it is read at its release RVA, matching the engine's own gate.
const HIK_ENABLE_FLAG_ADDRESS: usize = 0x142D621C8;

/// The offset of the per-character reduced-LOD flag byte within `CCharacter`: the engine skips the
/// IK pass when `(*(this + 10124) & 2) != 0` (verified in the release decompile of
/// `UpdatePassFinalizePose_Parallel`). Not modelled as a pyxis field, so it is read by raw offset.
const REDUCED_LOD_FLAG_OFFSET: usize = 10124;

/// The head-bone model-space orientations captured *before* the HumanIK solve, wired from the
/// pre-call [`character_update_pass_finalize_pose_parallel`] hook to the post-solve
/// [`character_update_prop_effects`] head override so the override composes its body-relative offset
/// onto the pure animated orientation rather than the IK-bent one (which would double-count the yaw
/// HumanIK already applied toward the target).
struct PreIkPose {
    head_orientation: glam::Quat,
    neck_orientation: glam::Quat,
}
static PRE_IK_POSE: Mutex<Option<PreIkPose>> = Mutex::new(None);

/// Pre-call seam for headset-driven upper-body IK. Runs at the entry of the pose-finalization pass,
/// *before* the HumanIK `MAIN` solve and its `HasTargets` gate (docs/engine/humanik.md): it captures the
/// headpose anchors from the freshly animated (pre-IK) pose and queues the head effector target so
/// the engine's own solver bends the spine and head toward the headpose the same frame. The
/// `SetJoint` head override still runs at the very end of this pass, in
/// [`character_update_prop_effects`], on top of the HIK-bent spine.
#[detour(
    address = jc3gi::character::character::Character::UpdatePassFinalizePose_Parallel_ADDRESS
)]
fn character_update_pass_finalize_pose_parallel(
    character: *mut Character,
    context: *mut std::ffi::c_void,
) {
    // Queue targets and capture anchors BEFORE the trampoline: the solve and the anchor-consuming
    // gate both run inside the real function, so entry-queued targets are solved this frame and the
    // pre-solve pose is the pure animation result.
    unsafe {
        capture_anchors_and_queue_body_ik(character);
    }

    CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL
        .get()
        .unwrap()
        .call(character, context);
}

/// Capture the headpose anchors (pre-IK) and queue the HumanIK head effector target for the local
/// player. Every engine-pointer hop is null-guarded; any failure leaves the previous anchors and
/// queues nothing.
unsafe fn capture_anchors_and_queue_body_ik(character: *mut Character) {
    unsafe {
        let Some(character) = character.as_mut().filter(|c| c.m_IsLocalCharacter) else {
            return;
        };
        let Some(animation_controller) = character.m_AnimatedModel.m_AnimationController.as_mut()
        else {
            return;
        };

        let head_index = character.GetSafeIndex(SafeBoneIndex::HEAD);
        let neck_index = character.GetSafeIndex(SafeBoneIndex::NECK);

        // Anchor capture, MOVED here from UpdatePropEffects. It must run pre-IK: the HumanIK MAIN
        // solve happens later in this same function, and reading the head bone *after* the solve
        // would let the anchor chase the very target HIK pulls it toward — a feedback loop that pins
        // the camera to a fixed world point while the body walks out from under it. Reading here,
        // before the solve, samples the freshly animated pose: the release decompile shows
        // AnimationController::GetJoint recomputes the model-space transform on demand when the bone
        // is dirty (CBlender::UpdateTime), and the animation graph finalized this frame's local pose
        // earlier in SIM, so this read reflects this frame's animation. At worst it is the previous
        // frame's model-space pose (one frame of latency) — still IK-free, and it can never contain
        // this frame's solve, so no tight feedback loop can form. The counterpart capture in
        // UpdatePropEffects is removed; that hook now only consumes the published anchors.
        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, character_rotation, _) = character_world.to_scale_rotation_translation();
        let joint_translation = |joint: &Joint| {
            let [x, y, z] = joint.m_Translation.data;
            glam::Vec3::new(x, y, z)
        };
        let quat_of = |joint: &Joint| {
            let [qx, qy, qz, qw] = joint.m_Orientation.data;
            glam::Quat::from_xyzw(qx, qy, qz, qw)
        };

        let mut head_joint = Joint::default();
        animation_controller.GetJoint(head_index, &mut head_joint);
        let animated_head_world = character_world.transform_point3(joint_translation(&head_joint));
        // The previous-tick head anchor: the same animated joint through the character's T0 world
        // matrix. Feeds the VR pose pair so the engine's sub-frame lerp smooths per-tick anchor
        // motion (vehicles, parachuting) instead of stepping the camera at the tick rate.
        let character_world_prev = glam::Mat4::from(character.m_WorldMatrixT0);
        let animated_head_world_prev =
            character_world_prev.transform_point3(joint_translation(&head_joint));

        let mut neck_joint = Joint::default();
        animation_controller.GetJoint(neck_index, &mut neck_joint);
        let animated_neck_world = character_world.transform_point3(joint_translation(&neck_joint));

        let eye_joint = |name: &[u8]| {
            let mut joint = Joint::default();
            animation_controller.GetJoint(
                animation_controller.GetBoneIndex(hashlittle(name) as u32),
                &mut joint,
            );
            joint_translation(&joint)
        };
        let eye_mid_model = (eye_joint(b"fLeftEye") + eye_joint(b"fRightEye")) / 2.0;
        let eye_mid_world = character_world.transform_point3(eye_mid_model);
        let eye_arm = character_rotation.inverse() * (eye_mid_world - animated_neck_world);

        headpose::set_anchors(headpose::Anchors {
            head: animated_head_world,
            head_prev: animated_head_world_prev,
            neck: animated_neck_world,
            eye_arm,
        });

        // Snapshot the pre-IK head and neck model-space orientations for the post-solve override's
        // compose base (see PreIkPose). Refreshed every frame the local player is active, so it can
        // never go stale.
        let animated_head_orientation = quat_of(&head_joint);
        *PRE_IK_POSE.lock() = Some(PreIkPose {
            head_orientation: animated_head_orientation,
            neck_orientation: quat_of(&neck_joint),
        });

        // From here on: the HumanIK body-follow targets, behind the body_ik config.
        let cfg = Config::lock_query(|c| c.body_ik);
        if !cfg.enabled || !headpose::is_active() || headpose::anchor().is_none() {
            return;
        }

        // Respect the same gates the engine checks before the HasTargets gate (docs/engine/humanik.md):
        // the global HIK enable and the per-character reduced-LOD bit. Queuing while gated off would
        // leave targets unconsumed (neither the solve nor ClearTargets runs), so skip cleanly and
        // log the transition once rather than per frame.
        let hik_globally_enabled = *(HIK_ENABLE_FLAG_ADDRESS as *const bool);
        let reduced_lod =
            *(character as *const Character as *const u8).add(REDUCED_LOD_FLAG_OFFSET) & 2 != 0;
        if !hik_globally_enabled || reduced_lod {
            log_gate_skip(hik_globally_enabled, reduced_lod);
            return;
        }
        GATE_SKIP_LOGGED.store(false, Ordering::Relaxed);

        // The head bone index maps to a HumanIK effector (expected 15); -1 means unmapped — skip.
        let effector = character.m_HIK.GetEffectorIdFromBoneIndex(head_index);
        if !(0..HumanIK::EFFECTOR_SLOTS as i32).contains(&effector) {
            log_unmapped_effector(effector);
            return;
        }
        UNMAPPED_LOGGED.store(false, Ordering::Relaxed);
        let eff = effector as usize;

        // The head world target is the headpose position (already anchored to this frame's animated
        // head plus the roomscale offset), transformed into character-model space — the space
        // AddEffectorTargetPosition expects (docs/engine/humanik.md) — plus the optional tuning offset.
        let target_world = headpose::query().position;
        let target_model =
            character_world.inverse().transform_point3(target_world) + cfg.target_offset;
        let pos = Vector3 {
            data: [target_model.x, target_model.y, target_model.z],
        };

        let weight = cfg.weight.clamp(0.0, 1.0);
        let reach_t = (cfg.head_reach_t * weight).clamp(0.0, 1.0);

        // Only the head effector's target and reach slots are written; other effectors (aim IK,
        // hands) are left untouched, so this coexists with the game's own MAIN-pass targets — the
        // pass's solve step is the max of all queued targets' steps.
        character.m_HIK.AddEffectorTargetPosition(
            effector,
            &pos,
            SolveStep::UPPER_BODY,
            Pass::MAIN,
            cfg.interpolation,
            cfg.interpolation_rate,
            cfg.blend_out,
            cfg.blend_out_rate,
        );
        character.m_HIK.m_TargetReachT[eff] = reach_t;

        let mut reach_r = 0.0;
        if cfg.rotation_target {
            // Aim the head's model-space frame at the headpose orientation, mirroring the aim IK's
            // AddEffectorTargetRotationVector(axis, angle) call. The delta is the model-space
            // rotation that turns the animated head orientation to the desired one; both are model
            // space, so no bone rest-frame knowledge is needed. Sourced from headpose::query()
            // (source-agnostic — identical under the VR pose source).
            let desired_head_model = character_rotation.inverse() * headpose::query().orientation;
            let delta = desired_head_model * animated_head_orientation.inverse();
            let (axis, angle) = delta.to_axis_angle();
            if angle.abs() > 1.0e-4 && axis.is_finite() {
                let axis_v = Vector3 {
                    data: [axis.x, axis.y, axis.z],
                };
                character.m_HIK.AddEffectorTargetRotationVector(
                    effector,
                    angle,
                    &axis_v,
                    SolveStep::UPPER_BODY,
                    Pass::MAIN,
                    cfg.interpolation,
                    cfg.interpolation_rate,
                    cfg.blend_out,
                    cfg.blend_out_rate,
                );
                reach_r = (cfg.head_reach_r * weight).clamp(0.0, 1.0);
                character.m_HIK.m_TargetReachR[eff] = reach_r;
            }
        }

        log_body_ik(target_model, effector, reach_t, reach_r);
    }
}

/// Once-per-second DEBUG line for a log-based playtest: the model-space target actually queued, the
/// resolved head effector id, and the reach weights actually written.
fn log_body_ik(target_model: glam::Vec3, effector: i32, reach_t: f32, reach_r: f32) {
    if throttle(&BODY_IK_LOG_AT, Duration::from_secs(1)) {
        tracing::debug!(
            target: BODY_IK_TARGET,
            effector,
            target_model = ?target_model,
            reach_t,
            reach_r,
            "queued head effector target",
        );
    }
}

/// Log once (not per frame) when the engine's IK gates skip the pass, so a gated-off session is
/// visible without spamming. Re-arms when the gate next passes.
fn log_gate_skip(hik_globally_enabled: bool, reduced_lod: bool) {
    if !GATE_SKIP_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::debug!(
            target: BODY_IK_TARGET,
            hik_globally_enabled,
            reduced_lod,
            "HumanIK gated off; skipping body-IK target queue",
        );
    }
}

/// Log once when the head bone has no effector mapping (-1). Re-arms when a valid effector resolves.
fn log_unmapped_effector(effector: i32) {
    if !UNMAPPED_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::debug!(
            target: BODY_IK_TARGET,
            effector,
            "head bone has no HumanIK effector mapping; skipping body-IK target queue",
        );
    }
}

/// Return `true` at most once per `interval`, updating the last-fire time stored in `at`.
fn throttle(at: &Mutex<Option<Instant>>, interval: Duration) -> bool {
    let mut guard = at.lock();
    let now = Instant::now();
    if guard.is_none_or(|last| now.duration_since(last) >= interval) {
        *guard = Some(now);
        true
    } else {
        false
    }
}

static BODY_IK_LOG_AT: Mutex<Option<Instant>> = Mutex::new(None);
static GATE_SKIP_LOGGED: AtomicBool = AtomicBool::new(false);
static UNMAPPED_LOGGED: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::character::character::Character::UpdatePropEffects_ADDRESS)]
fn character_update_prop_effects(character: *mut Character, dt: f32) {
    CHARACTER_UPDATE_PROP_EFFECTS
        .get()
        .unwrap()
        .call(character, dt);

    // Hide the player's head and drive its full pose from the headpose.
    unsafe {
        let Some(character) = character.as_mut().filter(|c| c.m_IsLocalCharacter) else {
            return;
        };
        let Some(animation_controller) = character.m_AnimatedModel.m_AnimationController.as_mut()
        else {
            return;
        };

        let head_index = character.GetSafeIndex(SafeBoneIndex::HEAD);

        // Publish the render-block head-hide inputs (the draws run on the render thread and only
        // load these): the player root pair for ownership, and the collapse set — the HEAD bone
        // plus the facial bones, so every vertex weighted anywhere on the head (face, eyes, ears,
        // and the hair riding HEAD) collapses. Invalid lookups are filtered by the publish (a
        // missing name must not collapse the root).
        // The instance-info pointers for exact draw ownership: each model instance's embedded
        // CRBIInfo is the `info` every one of its block draws receives.
        let rbi_infos: [usize; AnimatedModel::MODEL_INSTANCE_SLOTS as usize] =
            character.m_AnimatedModel.m_ModelInstances.map(|instance| {
                if instance == 0 {
                    0
                } else {
                    instance as usize + AnimatedModel::MODEL_INSTANCE_RBI_INFO_OFFSET as usize
                }
            });
        crate::hooks::graphics_engine::render_block::publish_player_rbi_infos(&rbi_infos);
        crate::hooks::graphics_engine::render_block::publish_player_root(
            glam::Mat4::from(character.m_WorldMatrixT0)
                .w_axis
                .truncate(),
            glam::Mat4::from(character.m_WorldMatrixT1)
                .w_axis
                .truncate(),
        );
        let bone = |name: &[u8]| animation_controller.GetBoneIndex(hashlittle(name) as u32);
        crate::hooks::graphics_engine::render_block::publish_collapse_bones(&[
            head_index,
            bone(b"offset_facialOrienter"),
            bone(b"fJaw"),
            bone(b"fMidLwrLip"),
            bone(b"fLeftMouthCorner"),
            bone(b"fRightMouthCorner"),
            bone(b"fNose"),
            bone(b"fMidUprLip"),
            bone(b"fUprLids"),
            bone(b"fLwrLids"),
            bone(b"fLeftBrowMidA"),
            bone(b"fRightBrowMidA"),
            bone(b"fLeftEye"),
            bone(b"fRightEye"),
            bone(b"fLeftEar"),
            bone(b"fRightEar"),
        ]);

        // HEAD: optionally the legacy scale-hide, plus the full headpose pose, in a single
        // SetJoint. The head/neck/eye anchors are captured PRE-IK in
        // `character_update_pass_finalize_pose_parallel`: this hook runs last, after the HumanIK
        // solve *and* after CalculateModelSpacePose, so a capture here would read the IK-solved head
        // and chase the very target HIK pulls it toward (the feedback loop). This hook only consumes
        // the published anchors and applies the head override on top of the solved pose.
        let hide_scale = crate::config::Config::lock_query(|c| c.camera.hide_head_scale);
        let scale = 0.001;
        let mut joint = Joint::default();
        animation_controller.GetJoint(head_index, &mut joint);

        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, character_rotation, _) = character_world.to_scale_rotation_translation();
        let joint_translation = |joint: &Joint| {
            let [x, y, z] = joint.m_Translation.data;
            glam::Vec3::new(x, y, z)
        };

        let neck_index = character.GetSafeIndex(SafeBoneIndex::NECK);
        let mut neck_joint = Joint::default();
        animation_controller.GetJoint(neck_index, &mut neck_joint);
        // The head-hide collapse target: the render side cannot read positions out of the
        // palette (the translation slots depend on each block's layout), so the model-space neck
        // point comes from the skeleton here.
        crate::hooks::graphics_engine::render_block::publish_collapse_target(joint_translation(
            &neck_joint,
        ));

        // The pre-IK head/neck orientations captured this frame before the solve. When body_ik is
        // driving the head, the joints read above are the *post-IK* pose, so composing the
        // override's body-relative offset onto their orientation would double-count the yaw HumanIK
        // already bent toward the target. Composing onto the pre-IK base instead makes the override
        // "set the exact head orientation" — identical to the no-IK case — with HIK's spine/neck
        // bend sitting underneath it. When body_ik is off the pre-IK and post-IK orientations match,
        // so the fallback to the freshly read orientation preserves the flatscreen path exactly.
        let body_ik_enabled = crate::config::Config::lock_query(|c| c.body_ik.enabled);
        let pre_ik = body_ik_enabled
            .then(|| {
                PRE_IK_POSE
                    .lock()
                    .as_ref()
                    .map(|p| (p.head_orientation, p.neck_orientation))
            })
            .flatten();

        if hide_scale {
            joint.m_Scale.data = [scale, scale, scale];
        }

        // Only override the pose once a valid anchor exists; until then (loading screens, garbage
        // bone data) the bone keeps its animated pose and only the legacy scale-hide applies.
        if headpose::is_active() && headpose::anchor().is_some() {
            // Compose the player's body-relative offset onto the *animated* model-space
            // orientation, exactly like the neck twist below. The previous absolute write assumed
            // the bone's rest frame matched the model axes, which it does not — observed in the
            // (now headful) shadow as the head collapsing into the shoulders. Model space is the
            // body frame, so the sim's body-relative angles apply directly, and the animated
            // translation is kept (plus the roomscale offset brought into the body frame), so the
            // head stays anatomically placed while turning where the player looks.
            let (yaw, pitch, roll) = headpose::sim::euler_angles();
            let offset_model = glam::Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll);
            let animated = pre_ik.map(|(head, _)| head).unwrap_or_else(|| {
                let [qx, qy, qz, qw] = joint.m_Orientation.data;
                glam::Quat::from_xyzw(qx, qy, qz, qw)
            });
            let composed = offset_model * animated;
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [composed.x, composed.y, composed.z, composed.w];

            // The roomscale positional offset, brought into the body frame. `animated_head_world`
            // is the pre-IK anchor published by the finalize hook, so the offset stays the pure
            // roomscale displacement even when HumanIK has moved the head. Zero whenever the offset
            // config is zero. (When body_ik drives the head toward `query().position` and a nonzero
            // offset is set, HIK's positional reach and this add both move the head toward the
            // offset — a known interaction, inert with the default zero offset.)
            let animated_head_world = headpose::anchor().unwrap_or(glam::Vec3::ZERO);
            let world_offset = headpose::query().position - animated_head_world;
            if world_offset != glam::Vec3::ZERO {
                let model_offset = character_rotation.inverse() * world_offset;
                let [tx, ty, tz] = joint.m_Translation.data;
                joint.m_Translation.data = [
                    tx + model_offset.x,
                    ty + model_offset.y,
                    tz + model_offset.z,
                ];
            }
        }

        animation_controller.SetJoint(head_index, &mut joint);

        // Twist the neck along with the head beyond the configured start: the head bone carries
        // the whole body-relative yaw, and past a real head's range the neck must follow or the
        // skinning between the (hidden) head and the animated neck knots up — this is what makes
        // the extended free-look yaw range anatomically plausible. Body-relative yaw is exactly a
        // model-space Y rotation, so the twist pre-multiplies the *animated* model-space neck
        // orientation captured above — no rest-frame knowledge needed, and the neck's translation
        // (its own origin) is untouched.
        if headpose::is_active() && headpose::anchor().is_some() {
            let (yaw, _, _) = headpose::sim::euler_angles();
            let (start_deg, max_deg) = Config::lock_query(|c| {
                (
                    c.headpose.neck_twist_start_deg,
                    c.headpose.neck_twist_max_deg,
                )
            });
            let excess_deg = (yaw.abs().to_degrees() - start_deg).clamp(0.0, max_deg.max(0.0));
            if excess_deg > 0.0 {
                let twist = excess_deg.to_radians().copysign(yaw);
                // Pre-multiply onto the pre-IK neck orientation for the same reason as the head
                // above: when body_ik has bent the neck toward the target, twisting the post-IK
                // orientation would compound HIK's neck rotation with this manual twist.
                let animated = pre_ik.map(|(_, neck)| neck).unwrap_or_else(|| {
                    let [qx, qy, qz, qw] = neck_joint.m_Orientation.data;
                    glam::Quat::from_xyzw(qx, qy, qz, qw)
                });
                let twisted = glam::Quat::from_rotation_y(twist) * animated;
                neck_joint.m_Orientation.data = [twisted.x, twisted.y, twisted.z, twisted.w];
                animation_controller.SetJoint(neck_index, &mut neck_joint);
            }
        }

        // Facial bones: scale only (the legacy head-hide behaviour).
        if !hide_scale {
            return;
        }
        let facial_indices = [
            // "offset_facialOrienter",
            "fJaw",
            "fMidLwrLip",
            "fLeftMouthCorner",
            "fRightMouthCorner",
            // "fNose",
            "fMidUprLip",
            // "fUprLids",
            // "fLwrLids",
            // "fLeftBrowMidA",
            // "fRightBrowMidA",
            // "fLeftEye",
            // "fRightEye",
            // "fLeftEar",
            // "fRightEar",
        ];
        for s in facial_indices {
            let index = animation_controller.GetBoneIndex(hashlittle(s.as_bytes()) as u32);
            let mut joint = Joint::default();
            animation_controller.GetJoint(index, &mut joint);
            joint.m_Scale.data = [scale, scale, scale];
            animation_controller.SetJoint(index, &mut joint);
        }
    }
}

/// Drop the vehicle reversing look-behind acts for the local player: `ACT_REVERSE` (cars) and
/// `ACT_REVERSE_MOTORBIKE` drive the `S_REVERSE_*` states in `rico_base.afsmb`, where Rico turns
/// to look over his shoulder. With a player-driven head, looking behind is the player's job, so
/// the act is swallowed and the driving pose stays forward; the state machine's rule system
/// drops the matching return transitions on its own (they only fire from the reverse states).
#[detour(address = jc3gi::character::character::Character::QueueAct_ADDRESS)]
fn character_queue_act(character: *mut Character, act: *const u32) {
    if Config::lock_query(|c| c.movement.suppress_reverse_look)
        && (unsafe { character.as_ref() }).is_some_and(|c| c.m_IsLocalCharacter)
        && let Some(&id) = (unsafe { act.as_ref() })
        && REVERSE_ACT_IDS.contains(&(id as i32))
    {
        return;
    }
    CHARACTER_QUEUE_ACT.get().unwrap().call(character, act);
}

/// The runtime ids of the reversing acts, resolved lazily from the event-id symbol table: act ids
/// are sequential registration indices, not name hashes, so they cannot be computed offline. The
/// singleton is unconditionally live by first use — the game creates the table during executable
/// initialization (the `ACT_*` id globals' dynamic initializers already resolve through it), and
/// the payload injects into a long-running process — so a missing table is a programming error
/// worth panicking on. Both names are registered by loaded animation data long before gameplay
/// queues acts, so the lookups are pure reads.
static REVERSE_ACT_IDS: LazyLock<[i32; 2]> = LazyLock::new(|| unsafe {
    let table = EventIdSymbolTable::get().expect("the animation event-id symbol table is not live");
    [
        EventIdSymbolTable::ACT_REVERSE,
        EventIdSymbolTable::ACT_REVERSE_MOTORBIKE,
    ]
    .map(|name| {
        let name = std::ffi::CString::new(name).expect("an act name contains a NUL");
        table.string_to_id(name.as_ptr() as *const u8)
    })
});
