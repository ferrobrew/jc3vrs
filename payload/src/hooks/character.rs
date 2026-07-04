use std::sync::LazyLock;

use detours_macro::detour;
use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::character::{AnimatedModel, Character, Joint, SafeBoneIndex},
    hash::hashlittle,
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, headpose};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
        .with_static_binder(&CHARACTER_QUEUE_ACT_BINDER)
}

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
        // SetJoint.
        let hide_scale = crate::config::Config::lock_query(|c| c.camera.hide_head_scale);
        let scale = 0.001;
        let mut joint = Joint::default();
        animation_controller.GetJoint(head_index, &mut joint);

        // Publish this frame's *animated* head and neck positions as the headpose anchors before
        // overriding the head bone. UpdatePropEffects runs after CalculateModelSpacePose, so the
        // joint reads are the freshly animated model-space pose, not last frame's override —
        // reading the bone matrices instead would return the override and freeze the anchor in
        // place (the feedback loop that pinned the camera to a fixed world point and stretched
        // the head toward it). The neck anchor gives the camera its pivot: pitching the head
        // swings the eyes about the neck instead of rotating in place at the skull base.
        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, character_rotation, _) = character_world.to_scale_rotation_translation();
        let joint_translation = |joint: &Joint| {
            let [x, y, z] = joint.m_Translation.data;
            glam::Vec3::new(x, y, z)
        };
        let animated_head_world = character_world.transform_point3(joint_translation(&joint));

        let neck_index = character.GetSafeIndex(SafeBoneIndex::NECK);
        let mut neck_joint = Joint::default();
        animation_controller.GetJoint(neck_index, &mut neck_joint);
        // The head-hide collapse target: the render side cannot read positions out of the
        // palette (the translation slots depend on each block's layout), so the model-space neck
        // point comes from the skeleton here.
        crate::hooks::graphics_engine::render_block::publish_collapse_target(joint_translation(
            &neck_joint,
        ));
        let animated_neck_world = character_world.transform_point3(joint_translation(&neck_joint));

        // The animated eye midpoint, expressed as a body-frame arm from the neck pivot: rotating
        // it by the published head orientation places the eyes anatomically as the head pitches.
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
            neck: animated_neck_world,
            eye_arm,
        });

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
            let [qx, qy, qz, qw] = joint.m_Orientation.data;
            let animated = glam::Quat::from_xyzw(qx, qy, qz, qw);
            let composed = offset_model * animated;
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [composed.x, composed.y, composed.z, composed.w];

            // The roomscale positional offset, brought into the body frame. Zero whenever the
            // offset config is zero: the pose position is the anchor captured above plus the
            // offset.
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
                let [qx, qy, qz, qw] = neck_joint.m_Orientation.data;
                let animated = glam::Quat::from_xyzw(qx, qy, qz, qw);
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
