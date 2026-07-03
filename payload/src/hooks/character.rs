use detours_macro::detour;
use jc3gi::{
    character::character::{Character, Joint, SafeBoneIndex},
    hash::hashlittle,
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
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

        let scale = 0.001;
        let head_index = character.GetSafeIndex(SafeBoneIndex::HEAD);

        // HEAD: override scale (head-hide) + full pose (headpose), in a single SetJoint.
        let mut joint = Joint::default();
        animation_controller.GetJoint(head_index, &mut joint);

        // Publish this frame's *animated* head position as the headpose anchor before overriding
        // the bone. UpdatePropEffects runs after CalculateModelSpacePose, so the joint read above
        // is the freshly animated model-space pose, not last frame's override — reading the bone
        // matrices instead would return the override and freeze the anchor in place (the feedback
        // loop that pinned the camera to a fixed world point and stretched the head toward it).
        let character_world = glam::Mat4::from(character.m_WorldMatrixT1);
        let [tx, ty, tz] = joint.m_Translation.data;
        let animated_head_world = character_world.transform_point3(glam::Vec3::new(tx, ty, tz));
        crate::headpose::set_anchor(animated_head_world);

        joint.m_Scale.data = [scale, scale, scale];

        // Only override the pose once a valid anchor exists; until then (loading screens, garbage
        // bone data) the bone keeps its animated pose and only the head-hide scale applies.
        if crate::headpose::is_active() && crate::headpose::anchor().is_some() {
            let headpose = crate::headpose::query();
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

        // Facial bones: scale only (the existing head-hide behaviour).
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
