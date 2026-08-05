use std::{
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::{
    animation::{
        ik::{Effector, HumanIK, Pass, RotationAxis, SolveStep},
        symbol_table::EventIdSymbolTable,
    },
    character::character::{
        AimState, AnimatedModel, Character, CharacterLodFlags, Joint, SafeBoneIndex,
        get_Character_EnableHIK,
    },
    hash::hashlittle,
    types::math::Vector3,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    headpose,
    hooks::{self, animation::active_state_type_id, graphics_engine::render_block},
};

pub(crate) mod config;
pub(crate) use config::BodyIkConfig;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_UPDATE_PASS_FINALIZE_POSE_PARALLEL_BINDER)
        .with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
        .with_static_binder(&CHARACTER_QUEUE_ACT_BINDER)
}

/// A snapshot of what the body-IK path did on its last frame, for the debug UI. The engine-side
/// control values are read at hook entry, so they show what the *previous* frame's drive and solve
/// actually left in the solver — the ground truth for whether a queued weight took effect.
#[derive(Clone, Copy, Default)]
pub(crate) struct BodyIkStatus {
    /// Why nothing was queued, or `None` if targets were queued.
    pub skip: Option<BodyIkSkip>,
    /// The headpose mode this frame: on foot versus everything else (vehicle, wingsuit, parachute).
    /// The body-IK response differs sharply between the two — on foot the character turns to follow
    /// the head, leaving the torso little to do — so a report of "nothing moved" means different
    /// things in each.
    pub mode: headpose::sim::HeadMode,
    /// The character's active animation rule-state id (`hashlittle(state_name)`), or `0` if the rule
    /// system is not readable. The release build keeps only the hash, so this identifies the state
    /// without naming it: compare two captures, or hash a candidate name to confirm a guess.
    pub state_id: u32,
    pub head_effector: i32,
    /// The chest-end effector id, resolved only while the torso drive is active; `-1` otherwise.
    pub chest_effector: i32,
    /// Whether the chest-end rotation target was actually queued this frame.
    pub chest_driven: bool,
    /// Model-space distance from the animated head bone to the queued head target.
    pub target_delta: f32,
    /// The lean component of the torso rotation, in degrees. Computed even when the drive stands
    /// down, so it reports what *would* queue.
    pub torso_lean_deg: f32,
    /// The yaw component of the torso rotation, in degrees; computed even when standing down.
    pub torso_yaw_deg: f32,
    /// How many shoulder effectors this rig's characterization actually maps to a bone (0, 1, or
    /// 2), independent of whether anything was queued for them.
    pub shoulders_mapped: u8,
    /// How many shoulder effectors got a positional target this frame (0, 1, or 2).
    pub shoulders_driven: u8,
    /// How many wrist effectors were pinned to their animated position this frame (0, 1, or 2).
    pub hands_pinned: u8,
    /// Whether the torso targets and hand pins stood down this frame because the character was
    /// aiming a weapon or the grapple. Distinguishes "deferring to the game's aim machinery" from
    /// "there was nothing to do": both report zero shoulders driven and zero hands pinned.
    pub deferred_to_aim: bool,
    /// The head's room-scale displacement from its animated anchor, in character-model space
    /// (metres): what the shoulder targets and the head's pull actually have to work with. Kept as a
    /// vector rather than a magnitude because the direction is the diagnostic — a lean that feels
    /// unresponsive is usually one whose displacement is small on the axis being tested, and a
    /// magnitude alone cannot show that.
    pub head_offset: glam::Vec3,
    /// The axis of the lean swing, in character-model space, or zero when there is no lean. Says
    /// which way the torso was asked to bend.
    pub lean_axis: glam::Vec3,
    pub engine_reach_t: f32,
    pub engine_reach_r: f32,
    pub engine_pull: f32,
}

impl BodyIkStatus {
    fn skipped(skip: BodyIkSkip) -> Self {
        Self {
            skip: Some(skip),
            mode: headpose::sim::mode(),
            ..Self::default()
        }
    }
}

/// The reason the body-IK path queued nothing on a frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyIkSkip {
    Disabled,
    HeadposeInactive,
    NoAnchor,
    NotInGameplay,
    HikDisabled,
    ReducedLod,
    UnmappedEffector,
}

impl BodyIkSkip {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled in config",
            Self::HeadposeInactive => "headpose inactive",
            Self::NoAnchor => "no head anchor yet",
            Self::NotInGameplay => "not in gameplay",
            Self::HikDisabled => "HumanIK globally disabled",
            Self::ReducedLod => "character in reduced LOD",
            Self::UnmappedEffector => "head bone has no effector",
        }
    }
}

/// The last frame's body-IK snapshot, or `None` before the hook has run once.
pub(crate) fn body_ik_status() -> Option<BodyIkStatus> {
    *BODY_IK_STATUS.lock()
}

fn set_body_ik_status(status: BodyIkStatus) {
    *BODY_IK_STATUS.lock() = Some(status);
}

static BODY_IK_STATUS: Mutex<Option<BodyIkStatus>> = Mutex::new(None);

/// The tracing target for the body-IK path. Public so the debug UI can offer a checkbox that turns
/// it up to `DEBUG` without the user composing a filter directive by hand.
pub(crate) const BODY_IK_TARGET: &str = "body_ik";

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
/// *before* the HumanIK `MAIN` solve and its `HasTargets` gate (docs/engine/character/humanik.md): it captures the
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

/// The skeleton bone an effector drives, or `None` if this rig's characterization has no node for
/// it.
///
/// Asking the solver rather than naming a bone ourselves. The bone-to-effector table is keyed by the
/// bone whose *name matches the HumanIK node's own name* (`Init` resolves `HIKNodeNameFromNodeId`
/// through `CSkeleton::GetBoneIndexByHashedName`), which is a different naming scheme from
/// `SafeBoneIndex`. The two happen to agree for the head, which is why the head effector resolves
/// from `GetSafeIndex(HEAD)` — but not for the shoulders, where the game's shoulder safe bone is not
/// the bone HumanIK calls a shoulder, so the lookup returned `-1` and the shoulder targets were
/// silently never queued. Scanning `m_HIKNodeAndBonePairs` inverts the same relation with no name
/// assumption at all, and yields the bone index needed to read the effector's animated pose.
unsafe fn bone_for_effector(hik: &HumanIK, effector: i32) -> Option<u32> {
    unsafe {
        let pairs = &hik.m_HIKNodeAndBonePairs;
        let (mut cur, end) = (pairs.begin, pairs.end);
        while !cur.is_null() && cur < end {
            let bone = (*cur).bone_index;
            if bone >= 0 && hik.GetEffectorIdFromBoneIndex(bone as u32) == effector {
                return Some(bone as u32);
            }
            cur = cur.add(1);
        }
        None
    }
}

/// Whether the game has queued a live `MAIN`-pass target for `effector` *this frame*.
///
/// The game's own systems queue during animation-graph evaluation, well before this hook runs at the
/// entry of the pose-finalization pass, so their targets are already in the pass's lists and can be
/// seen. That matters because a second target for the same effector *updates* the first rather than
/// coexisting with it: writing a wrist target on a frame when `NRightArmAimIK` has aimed that wrist
/// replaces the aim, and since the weapon rides a hand attach bone, the gun stops pointing where the
/// player is aiming.
///
/// Only entries whose `is_valid` flag is set count: `ClearTargets` retains consumed entries until
/// their reach decays, so presence alone means "targeted *recently*" — most likely by this hook on
/// the previous frame. Matching on presence made every pin block its own re-queue, steering the
/// hands with a stale ghost for the blend-out tail. The game re-queues its active targets every
/// frame, so a set flag is exactly its live claim.
///
/// The policy this enables is deliberately asymmetric. For the arms, hands, and chest the game's
/// target expresses functional intent — aiming a weapon, gripping, swinging toward a grapple hook —
/// and the mod defers. The head is the exception: the HMD owns it by definition, so the head target
/// is written even though the aim IK drives that effector too.
unsafe fn effector_live_targeted(hik: &HumanIK, effector: i32) -> bool {
    unsafe {
        let pass = &hik.m_PassInfo[Pass::MAIN as usize];
        let positions = &pass.m_EffectorTargetPositions;
        let rotations = &pass.m_EffectorTargetRotations;
        let mut cur = positions.begin;
        while !cur.is_null() && cur < positions.end {
            if (*cur).effector == effector && (*cur).is_valid {
                return true;
            }
            cur = cur.add(1);
        }
        let mut cur = rotations.begin;
        while !cur.is_null() && cur < rotations.end {
            if (*cur).effector == effector && (*cur).is_valid {
                return true;
            }
            cur = cur.add(1);
        }
        false
    }
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
        *PRE_IK_POSE.lock() = Some(PreIkPose {
            head_orientation: quat_of(&head_joint),
            neck_orientation: quat_of(&neck_joint),
        });

        // From here on: the HumanIK body-follow targets, behind the body_ik config. Only while
        // gameplay owns the character: outside E_GAME_RUN the engine repositions Rico for a teleport,
        // so queuing head-follow targets would fight that (issue #27). The anchor capture above still
        // runs so the pose stays fresh for the auto-recenter rebase on resume.
        let cfg = Config::lock_query(|c| c.body_ik);
        if let Some(skip) = (!cfg.enabled)
            .then_some(BodyIkSkip::Disabled)
            .or((!headpose::is_active()).then_some(BodyIkSkip::HeadposeInactive))
            .or(headpose::anchor().is_none().then_some(BodyIkSkip::NoAnchor))
            .or((!hooks::in_gameplay()).then_some(BodyIkSkip::NotInGameplay))
        {
            set_body_ik_status(BodyIkStatus::skipped(skip));
            return;
        }

        // Respect the same gates the engine checks before the HasTargets gate (docs/engine/character/humanik.md):
        // the global HIK enable and the per-character reduced-LOD bit. Queuing while gated off would
        // leave targets unconsumed (neither the solve nor ClearTargets runs), so skip cleanly and
        // log the transition once rather than per frame.
        let hik_globally_enabled = *get_Character_EnableHIK();
        let reduced_lod = character
            .m_LodFlags
            .contains(CharacterLodFlags::REDUCED_LOD);
        if !hik_globally_enabled || reduced_lod {
            log_gate_skip(hik_globally_enabled, reduced_lod);
            set_body_ik_status(BodyIkStatus::skipped(if reduced_lod {
                BodyIkSkip::ReducedLod
            } else {
                BodyIkSkip::HikDisabled
            }));
            return;
        }
        GATE_SKIP_LOGGED.store(false, Ordering::Relaxed);

        // The head bone index maps to a HumanIK effector (expected 15); -1 means unmapped — skip.
        let effector = character.m_HIK.GetEffectorIdFromBoneIndex(head_index);
        if !(0..HumanIK::EFFECTOR_SLOTS as i32).contains(&effector) {
            log_unmapped_effector(effector);
            set_body_ik_status(BodyIkStatus::skipped(BodyIkSkip::UnmappedEffector));
            return;
        }
        UNMAPPED_LOGGED.store(false, Ordering::Relaxed);
        let eff = effector as usize;

        // The head world target is the headpose position (already anchored to this frame's animated
        // head plus the roomscale offset), transformed into character-model space — the space
        // AddEffectorTargetPosition expects (docs/engine/character/humanik.md) — plus the optional tuning offset.
        let target_world = headpose::query().position;
        let target_model =
            character_world.inverse().transform_point3(target_world) + cfg.target_offset;
        let pos = Vector3 {
            data: [target_model.x, target_model.y, target_model.z],
        };

        let weight = cfg.weight.clamp(0.0, 1.0);
        let reach_t = (cfg.head_reach_t * weight).clamp(0.0, 1.0);
        let pull = (cfg.head_pull * weight).clamp(0.0, 1.0);

        // Only the head effector's control slots are written; other effectors (aim IK, hands) are
        // left untouched, so this coexists with the game's own MAIN-pass targets — the pass's solve
        // step is the max of all queued targets' steps.
        //
        // SPINE_HEAD_ONLY, not UPPER_BODY: a step that admits the arms lets the solver reach a
        // moved head by swinging them instead of bending the spine. The torso targets below
        // re-admit the arms at UPPER_BODY — exactly the frames where the hand pins hold them.
        character.m_HIK.AddEffectorTargetPosition(
            effector,
            &pos,
            SolveStep::SPINE_HEAD_ONLY,
            Pass::MAIN,
            cfg.interpolation,
            cfg.interpolation_rate,
            cfg.blend_out,
            cfg.blend_out_rate,
        );
        character.m_HIK.m_TargetReachT[eff] = reach_t;
        // Reach alone makes the head *arrive* at the target; pull is what propagates that
        // requirement into the shoulders, chest, and spine. Without it the neck absorbs the whole
        // delta and the torso never moves — the effect this feature exists to produce. The engine
        // copies `m_TargetPull` into `m_Pull` for every effector carrying a queued *position*
        // target (docs/engine/character/humanik.md), so it must be written alongside the position
        // target, not the rotation one.
        character.m_HIK.m_TargetPull[eff] = pull;

        // The torso articulation. A lean is a rotation of the spine about its base, not a
        // translation of the girdle: displacing both shoulders by a fixed fraction of the head's
        // offset is a linearization that only holds for small lateral leans, and off that plane it
        // telescopes the spine (a head dropped 30 cm would drive the shoulders 24 cm straight down
        // into the ribcage) and can never rotate the girdle, since both shoulders receive the same
        // displacement. So one torso rotation is derived about a pivot at the base of the spine, and
        // everything downstream is expressed through it.
        //
        // Its two components:
        //
        // - The **lean**: the swing about the pivot that carries the animated head to where the
        //   player's head actually is, as an arc rather than a straight line. Because it is a
        //   rotation, a lateral offset swings the girdle sideways *and* counter-rotates it, a
        //   forward offset hinges the torso forward, and a downward offset flexes it — each falling
        //   out of the same construction instead of needing its own case. What a rotation cannot
        //   represent is the head changing its *distance* from the pivot, and dropping that residual
        //   is correct: a spine does not telescope. The neck and the head effector's own reach
        //   absorb it.
        // - The **yaw**: a share of how far the head is turned from the body's facing, about the
        //   pivot's vertical axis.
        // The pivot, somewhere between the base of the spine and the hips. Which one is right
        // depends on the direction of the lean, and a single point cannot be right for both:
        // leaning *sideways* is genuinely spinal, so the spine's base is the joint that bends, but
        // leaning *forward* is mostly a hip hinge, and rotating about a point up in the back tips the
        // chest without carrying the torso forward the way a hinge would. Pivoting lower trades angle
        // for travel — the same head displacement resolves to a smaller rotation about a longer arm,
        // so the shoulders move further. `pivot_drop` picks the blend; the anisotropic version, with
        // the pivot chosen per axis, is the follow-up if this confirms the theory.
        let joint_position = |bone: u32| {
            let mut joint = Joint::default();
            animation_controller.GetJoint(bone, &mut joint);
            joint_translation(&joint)
        };
        let spine_pivot = joint_position(character.GetSafeIndex(SafeBoneIndex::SPINE));
        let pivot = bone_for_effector(&character.m_HIK, Effector::HIPS as i32)
            .map_or(spine_pivot, |hips| {
                spine_pivot.lerp(joint_position(hips), cfg.pivot_drop.clamp(0.0, 1.0))
            });
        let head_offset_model = character_rotation.inverse()
            * (target_world - headpose::anchor().unwrap_or(animated_head_world));
        let lean = {
            let from = joint_translation(&head_joint) - pivot;
            let to = from + head_offset_model;
            // Degenerate only if the head sits on the pivot or the offset would drive it there,
            // neither of which is a real pose; the identity is the right answer for both.
            if from.length_squared() > 1.0e-8 && to.length_squared() > 1.0e-8 {
                let swing = glam::Quat::from_rotation_arc(from.normalize(), to.normalize());
                let (axis, angle) = swing.to_axis_angle();
                // The cap is what keeps this model inside the range it is valid over, and it is a
                // real anatomical limit besides. As the head approaches the pivot's height the two
                // vectors approach antiparallel, where the swing angle runs to 180 degrees about an
                // axis that is barely determined — a head dropped below the base of the spine would
                // otherwise fold the torso over backwards, picking a near-arbitrary direction to do
                // it in. Saturating is still the wrong pose for a deep crouch (that is lower-body
                // articulation, not a spine bend — see docs/mod/body/head-and-body.md), but it is
                // bounded and stable rather than a flip.
                let limit = cfg.lean_max_deg.max(0.0).to_radians();
                let angle = (angle * cfg.lean_share.clamp(0.0, 1.0)).clamp(-limit, limit);
                if angle.abs() > 1.0e-4 && axis.is_finite() {
                    glam::Quat::from_axis_angle(axis, angle)
                } else {
                    glam::Quat::IDENTITY
                }
            } else {
                glam::Quat::IDENTITY
            }
        };
        // The yaw comes from the body-relative rotation rather than from a difference against the
        // animated head *bone*, whose rest orientation need not match the model axes: that
        // difference carries a constant bias that would twist the torso even with the head neutral.
        // The body-relative rotation is the identity at neutral by construction.
        let torso_yaw =
            headpose::sim::body_yaw_of(headpose::body_relative_rotation()) * cfg.yaw_share;
        let torso = lean * glam::Quat::from_rotation_y(torso_yaw);

        // While aiming, the torso targets and hand pins stand down wholesale (the head target
        // remains). The aim IK claims only the one arm effector it rotates but needs the whole
        // chain free to swing — per-effector deferral left the other pins clamping aim to the
        // authored sweep cone (~±30°).
        let defer_to_aim = cfg.defer_while_aiming
            && character
                .m_AimFlags
                .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple);
        let drive_torso = !torso.is_near_identity() && !defer_to_aim;

        // The girdle is then *told where to be*, rather than left to the solver. Relying on the head
        // effector's pull to carry the torso along does not work: the solver has cheaper ways to
        // reach a moved head — bending the neck, swinging the arms — and pull does not arbitrate
        // between them, it only sets how far the requirement travels. In a vehicle it took the arms,
        // which read as the hands leaving the wheel while the shoulders stayed welded to the seat.
        // Each shoulder is carried through the torso rotation about the pivot, so the girdle follows
        // the arc the spine would actually describe. Positions rather than orientations, so no bone
        // rest-frame knowledge is involved, and the same reach-plus-pull recipe the engine's own foot
        // and hip IK uses for a bone it wants placed.
        // Resolved before the neutral-torso guard so the debug readout can tell "this rig has no
        // shoulder nodes" apart from "there was nothing to do this frame" — the two look identical
        // from a count of targets queued, and the first masqueraded as the second for a while.
        let shoulder_bones = [
            Effector::LEFT_SHOULDER as i32,
            Effector::RIGHT_SHOULDER as i32,
        ]
        .map(|effector| (effector, bone_for_effector(&character.m_HIK, effector)));
        let shoulders_mapped = shoulder_bones.iter().filter(|(_, b)| b.is_some()).count() as u8;

        let mut shoulders_driven = 0;
        if drive_torso {
            let shoulder_reach_t = (cfg.shoulder_reach_t * weight).clamp(0.0, 1.0);
            let shoulder_pull = (cfg.shoulder_pull * weight).clamp(0.0, 1.0);
            for (shoulder_effector, index) in shoulder_bones {
                let Some(index) = index else {
                    continue;
                };
                if effector_live_targeted(&character.m_HIK, shoulder_effector) {
                    continue;
                }
                let mut joint = Joint::default();
                animation_controller.GetJoint(index, &mut joint);
                let target = pivot + torso * (joint_translation(&joint) - pivot);
                let target = Vector3 {
                    data: [target.x, target.y, target.z],
                };
                character.m_HIK.AddEffectorTargetPosition(
                    shoulder_effector,
                    &target,
                    SolveStep::UPPER_BODY,
                    Pass::MAIN,
                    cfg.interpolation,
                    cfg.interpolation_rate,
                    cfg.blend_out,
                    cfg.blend_out_rate,
                );
                character.m_HIK.m_TargetReachT[shoulder_effector as usize] = shoulder_reach_t;
                character.m_HIK.m_TargetPull[shoulder_effector as usize] = shoulder_pull;
                shoulders_driven += 1;
            }
        }

        // The chest end takes the same rotation as its orientation target, so the two channels can
        // never disagree: the shoulder positions say where the girdle goes, this says how it is
        // turned once it gets there. Queued as one combined rotation rather than two — a second
        // target for the same effector *updates* the first, so a separate lean and yaw target would
        // silently drop one of them.
        let mut chest_effector = -1;
        let mut chest_driven = false;
        if drive_torso {
            chest_effector = character.m_HIK.GetChestEndEffectorId();
            if (0..HumanIK::EFFECTOR_SLOTS as i32).contains(&chest_effector)
                && !effector_live_targeted(&character.m_HIK, chest_effector)
            {
                let (axis, angle) = torso.to_axis_angle();
                if angle.abs() > 1.0e-4 && axis.is_finite() {
                    let axis_v = Vector3 {
                        data: [axis.x, axis.y, axis.z],
                    };
                    character.m_HIK.AddEffectorTargetRotationVector(
                        chest_effector,
                        angle,
                        &axis_v,
                        SolveStep::UPPER_BODY,
                        Pass::MAIN,
                        cfg.interpolation,
                        cfg.interpolation_rate,
                        cfg.blend_out,
                        cfg.blend_out_rate,
                    );
                    character.m_HIK.m_TargetReachR[chest_effector as usize] =
                        (cfg.chest_reach_r * weight).clamp(0.0, 1.0);
                    // A rotation target promotes `m_TargetResist` to live resist, so the slot
                    // must be written or a past writer's value leaks in.
                    character.m_HIK.m_TargetResist[chest_effector as usize] = 0.0;
                    chest_driven = true;
                }
            }
        }

        // Pin the hands where the animation put them. Moving the shoulders drags the arms with them,
        // because nothing else holds the hands: gripping a steering wheel is animation, not a
        // constraint the solver knows about, so leaning slid the hands off the wheel. Targeting each
        // wrist at its own *animated* position turns the grip into a constraint the arms have to
        // absorb — the shoulders move, the elbows and shoulders articulate, and the hands stay put,
        // which is the whole point of an upper body that follows the head. Pull is deliberately zero:
        // a pinned hand must not drag the torso back toward it, or it would cancel the very
        // displacement the shoulder targets just asked for.
        // Only while the girdle is driven: with neither shoulders nor chest queued the head's
        // SPINE_HEAD_ONLY step keeps the arms out of the solve entirely, and a standing wrist
        // target would only compete with the game's own grip IK.
        let mut hands_pinned = 0;
        if cfg.pin_hands && (shoulders_driven > 0 || chest_driven) {
            let hand_reach_t = (cfg.hand_reach_t * weight).clamp(0.0, 1.0);
            for hand_effector in [Effector::LEFT_WRIST, Effector::RIGHT_WRIST] {
                let hand_effector = hand_effector as i32;
                let Some(index) = bone_for_effector(&character.m_HIK, hand_effector) else {
                    continue;
                };
                // The weapon-aim IK drives the gun hand; overriding it points the gun somewhere the
                // player is not aiming.
                if effector_live_targeted(&character.m_HIK, hand_effector) {
                    continue;
                }
                let mut joint = Joint::default();
                animation_controller.GetJoint(index, &mut joint);
                let animated = joint_translation(&joint);
                let target = Vector3 {
                    data: [animated.x, animated.y, animated.z],
                };
                character.m_HIK.AddEffectorTargetPosition(
                    hand_effector,
                    &target,
                    SolveStep::UPPER_BODY,
                    Pass::MAIN,
                    cfg.interpolation,
                    cfg.interpolation_rate,
                    cfg.blend_out,
                    cfg.blend_out_rate,
                );
                character.m_HIK.m_TargetReachT[hand_effector as usize] = hand_reach_t;
                character.m_HIK.m_TargetPull[hand_effector as usize] = 0.0;

                // Position alone is not a grip. Holding only *where* the wrist is leaves the solver
                // free to satisfy that from any wrist orientation, so the hands stayed on the wheel
                // but spun in place — and since the weapon rides a hand attach bone, the aim went
                // with them. A rotation target pins the orientation too.
                //
                // A **zero** angle is the whole trick: the queued rotation is an offset applied to
                // the effector's *current* state, and `UpdateEffectorsFromTargets` samples that from
                // the pre-solve character pose. So an offset of nothing means "target the animated
                // orientation" — exactly the hold wanted, with no need to know what the solve will
                // do. Queuing it also puts this effector on the rotation list, which is the only
                // list that drives resist, so the hands' resistance to being dragged by the
                // shoulders' pull becomes available at all.
                character.m_HIK.AddEffectorTargetRotation(
                    hand_effector,
                    0.0,
                    RotationAxis::Y,
                    SolveStep::UPPER_BODY,
                    Pass::MAIN,
                    cfg.interpolation,
                    cfg.interpolation_rate,
                    cfg.blend_out,
                    cfg.blend_out_rate,
                );
                character.m_HIK.m_TargetReachR[hand_effector as usize] =
                    (cfg.hand_reach_r * weight).clamp(0.0, 1.0);
                character.m_HIK.m_TargetResist[hand_effector as usize] =
                    (cfg.hand_resist * weight).clamp(0.0, 1.0);
                hands_pinned += 1;
            }
        }

        // The engine applies a queued rotation as an offset onto the pre-solve state, so the
        // body-relative rotation targets exactly the orientation the `UpdatePropEffects` override
        // will set. Not derived from `headpose::query().orientation`: that folds in the grapple
        // filter's body frame, and dividing out the world rotation assumes the bone's rest frame
        // matches the model axes.
        let head_delta = headpose::body_relative_rotation();

        let mut reach_r = 0.0;
        if cfg.rotation_target {
            // Aim the head's model-space frame at the headpose orientation, mirroring the aim IK's
            // AddEffectorTargetRotationVector(axis, angle) call. SPINE_HEAD_ONLY for the same
            // reason as the position target above: turning the head must never recruit the arms.
            let (axis, angle) = head_delta.to_axis_angle();
            if angle.abs() > 1.0e-4 && axis.is_finite() {
                let axis_v = Vector3 {
                    data: [axis.x, axis.y, axis.z],
                };
                character.m_HIK.AddEffectorTargetRotationVector(
                    effector,
                    angle,
                    &axis_v,
                    SolveStep::SPINE_HEAD_ONLY,
                    Pass::MAIN,
                    cfg.interpolation,
                    cfg.interpolation_rate,
                    cfg.blend_out,
                    cfg.blend_out_rate,
                );
                reach_r = (cfg.head_reach_r * weight).clamp(0.0, 1.0);
                character.m_HIK.m_TargetReachR[eff] = reach_r;
                // Resist is the rotation list's counterpart to pull: the engine copies
                // `m_TargetResist` into `m_Resist` only for effectors carrying a queued *rotation*
                // target, so it belongs here rather than beside the position target above.
                character.m_HIK.m_TargetResist[eff] = (cfg.head_resist * weight).clamp(0.0, 1.0);
            }
        }

        let (lean_axis, lean_angle) = lean.to_axis_angle();
        let status = BodyIkStatus {
            skip: None,
            mode: headpose::sim::mode(),
            state_id: active_state_type_id(character).unwrap_or(0),
            head_effector: effector,
            chest_effector,
            chest_driven,
            // The positional delta the solver actually has to work with: the queued target against
            // the animated head bone it starts from. Near zero means the position target and its
            // pull can do nothing, whatever the weights say.
            target_delta: target_model.distance(joint_translation(&head_joint)),
            torso_lean_deg: lean_angle.to_degrees(),
            torso_yaw_deg: torso_yaw.to_degrees(),
            shoulders_mapped,
            shoulders_driven,
            hands_pinned,
            deferred_to_aim: defer_to_aim,
            head_offset: head_offset_model,
            lean_axis: if lean_angle > 1.0e-4 {
                lean_axis
            } else {
                glam::Vec3::ZERO
            },
            engine_reach_t: character.m_HIK.m_ReachT[eff],
            engine_reach_r: character.m_HIK.m_ReachR[eff],
            engine_pull: character.m_HIK.m_Pull[eff],
        };
        log_body_ik(&status, reach_t, reach_r, pull);
        set_body_ik_status(status);
    }
}

/// Once-per-second DEBUG line carrying the same picture as the debug UI's status readout, for
/// reading a session back afterwards rather than watching a panel mid-motion. Enabled by the Body IK
/// section's log checkbox, or a `body_ik=debug` filter directive.
fn log_body_ik(status: &BodyIkStatus, reach_t: f32, reach_r: f32, pull: f32) {
    if throttle(&BODY_IK_LOG_AT, Duration::from_secs(1)) {
        tracing::debug!(
            target: BODY_IK_TARGET,
            mode = ?status.mode,
            state_id = format_args!("{:#010x}", status.state_id),
            head_effector = status.head_effector,
            chest_effector = status.chest_effector,
            chest_driven = status.chest_driven,
            target_delta = status.target_delta,
            head_offset = ?status.head_offset,
            head_offset_len = status.head_offset.length(),
            lean_axis = ?status.lean_axis,
            torso_lean_deg = status.torso_lean_deg,
            torso_yaw_deg = status.torso_yaw_deg,
            shoulders_mapped = status.shoulders_mapped,
            shoulders_driven = status.shoulders_driven,
            hands_pinned = status.hands_pinned,
            deferred_to_aim = status.deferred_to_aim,
            reach_t,
            reach_r,
            pull,
            engine_reach_t = status.engine_reach_t,
            engine_reach_r = status.engine_reach_r,
            engine_pull = status.engine_pull,
            "queued body-IK targets",
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
        render_block::publish_player_rbi_infos(&rbi_infos);
        render_block::publish_player_root(
            glam::Mat4::from(character.m_WorldMatrixT0)
                .w_axis
                .truncate(),
            glam::Mat4::from(character.m_WorldMatrixT1)
                .w_axis
                .truncate(),
        );
        let bone = |name: &[u8]| animation_controller.GetBoneIndex(hashlittle(name) as u32);
        render_block::publish_collapse_bones(&[
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
        let hide_scale = Config::lock_query(|c| c.camera.hide_head_scale);
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
        render_block::publish_collapse_target(joint_translation(&neck_joint));

        // The pre-IK head/neck orientations captured this frame before the solve. When body_ik is
        // driving the head, the joints read above are the *post-IK* pose, so composing the
        // override's body-relative offset onto their orientation would double-count the yaw HumanIK
        // already bent toward the target. Composing onto the pre-IK base instead makes the override
        // "set the exact head orientation" — identical to the no-IK case — with HIK's spine/neck
        // bend sitting underneath it. When body_ik is off the pre-IK and post-IK orientations match,
        // so the fallback to the freshly read orientation preserves the flatscreen path exactly.
        let body_ik = Config::lock_query(|c| c.body_ik);
        let pre_ik = body_ik
            .enabled
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

        // Only drive the head/neck from the headpose while gameplay owns the character. Outside
        // E_GAME_RUN the engine repositions Rico for a teleport (issue #27), so the mod stops driving
        // his pose and lets the animation/engine place him; the head-hide above still applies.
        let in_gameplay = hooks::in_gameplay();

        // Only override the pose once a valid anchor exists; until then (loading screens, garbage
        // bone data) the bone keeps its animated pose and only the legacy scale-hide applies.
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            // Compose the player's body-relative offset onto the *animated* model-space
            // orientation, exactly like the neck twist below. The previous absolute write assumed
            // the bone's rest frame matched the model axes, which it does not — observed in the
            // (now headful) shadow as the head collapsing into the shoulders. Model space is the
            // body frame, so the sim's body-relative angles apply directly, and the animated
            // translation is kept (plus the roomscale offset brought into the body frame), so the
            // head stays anatomically placed while turning where the player looks.
            let offset_model = headpose::body_relative_rotation();
            let animated = pre_ik.map(|(head, _)| head).unwrap_or_else(|| {
                let [qx, qy, qz, qw] = joint.m_Orientation.data;
                glam::Quat::from_xyzw(qx, qy, qz, qw)
            });
            let composed = offset_model * animated;
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [composed.x, composed.y, composed.z, composed.w];

            // The roomscale positional offset, brought into the body frame — but only when HumanIK
            // is not already placing the head: both paths aim at the same tracked position, so
            // applying both displaces it twice. "Placing" requires an effective reach, not just a
            // queued target — with the reach dialled to zero the solve moves nothing and this add
            // must take over, or roomscale placement silently dies.
            let hik_places_head = body_ik.enabled
                && body_ik.head_reach_t * body_ik.weight > 0.0
                && body_ik_status().is_some_and(|s| s.skip.is_none());
            let animated_head_world = headpose::anchor().unwrap_or(glam::Vec3::ZERO);
            let world_offset = headpose::query().position - animated_head_world;
            if !hik_places_head && world_offset != glam::Vec3::ZERO {
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
        if in_gameplay && headpose::is_active() && headpose::anchor().is_some() {
            let yaw = headpose::sim::body_yaw_of(headpose::body_relative_rotation());
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
        && reverse_act_ids().is_some_and(|ids| ids.contains(&(id as i32)))
    {
        return;
    }
    CHARACTER_QUEUE_ACT.get().unwrap().call(character, act);
}

/// The runtime ids of the reversing acts (`ACT_REVERSE` / `ACT_REVERSE_MOTORBIKE`): act ids are
/// sequential registration indices, not name hashes, so they cannot be computed offline. Resolved
/// the first time the event-id symbol table is live and cached thereafter; `None` until then — after
/// a startup injection the table may not exist yet, and the suppression simply does not fire until
/// it does (the acts are registered by loaded animation data before gameplay queues them).
fn reverse_act_ids() -> Option<[i32; 2]> {
    if let Some(ids) = REVERSE_ACT_IDS.get() {
        return Some(*ids);
    }
    let ids = [
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE)?,
        resolve_act_id(EventIdSymbolTable::ACT_REVERSE_MOTORBIKE)?,
    ];
    Some(*REVERSE_ACT_IDS.get_or_init(|| ids))
}

static REVERSE_ACT_IDS: OnceLock<[i32; 2]> = OnceLock::new();

/// Resolve an `ACT_*` name to its runtime id via the event-id symbol table, or `None` if the table
/// is not live yet (e.g. immediately after a startup injection, before the animation system has
/// created it). Once the table exists the lookup is a pure read: the `ACT_*` names are registered by
/// loaded animation data before gameplay queues acts.
fn resolve_act_id(name: &std::ffi::CStr) -> Option<i32> {
    let table = unsafe { EventIdSymbolTable::get() }?;
    Some(unsafe { table.string_to_id(name.as_ptr() as *const u8) })
}
