use std::sync::LazyLock;

use detours_macro::detour;
use jc3gi::{
    animation::symbol_table::EventIdSymbolTable,
    character::character::{Character, Joint, SafeBoneIndex},
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

        // Publish the facial classification bones for the render-block head-hide (the draws run
        // on the render thread and only load these).
        crate::hooks::graphics_engine::render_block::publish_facial_bones([
            animation_controller.GetBoneIndex(hashlittle(b"fJaw") as u32) as u32,
            animation_controller.GetBoneIndex(hashlittle(b"fLeftEye") as u32) as u32,
            animation_controller.GetBoneIndex(hashlittle(b"fRightEye") as u32) as u32,
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
        // bone data) the bone keeps its animated pose and only the head-hide scale applies.
        if headpose::is_active() && headpose::anchor().is_some() {
            let headpose = headpose::query();
            let desired_head_world = headpose.to_mat4();
            let desired_head_model = character_world.inverse() * desired_head_world;
            let (_, rotation, translation) = desired_head_model.to_scale_rotation_translation();
            // Always write both translation and orientation: we take full control of the head bone
            // to match it to the player's head, as VR will.
            joint.m_Translation.data = [translation.x, translation.y, translation.z];
            // glam Quat (x,y,z,w) -> Havok AlignedQuat [x,y,z,w] is a direct copy.
            joint.m_Orientation.data = [rotation.x, rotation.y, rotation.z, rotation.w];
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
        table.string_to_id(c"ACT_REVERSE".as_ptr() as *const u8),
        table.string_to_id(c"ACT_REVERSE_MOTORBIKE".as_ptr() as *const u8),
    ]
});
