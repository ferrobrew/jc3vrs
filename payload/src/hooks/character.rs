use detours_macro::detour;
use jc3gi::{
    character::character::{Character, Joint, SafeBoneIndex},
    hash::hashlittle,
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CHARACTER_UPDATE_PROP_EFFECTS_BINDER)
}

#[detour(address = 0x143_AC2_390)]
fn character_update_prop_effects(character: *mut Character) {
    CHARACTER_UPDATE_PROP_EFFECTS.get().unwrap().call(character);

    // Hide the player's head
    unsafe {
        let Some(character) = character.as_mut().filter(|c| c.m_IsLocalCharacter) else {
            return;
        };

        let Some(animation_controller) = character.m_AnimatedModel.m_AnimationController.as_mut()
        else {
            return;
        };

        let scale = 0.001;

        let mut indices = vec![character.get_safe_index(SafeBoneIndex::HEAD)];
        indices.extend(
            [
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
            ]
            .iter()
            .map(|s: &&str| animation_controller.get_bone_index(hashlittle(s.as_bytes()) as u32)),
        );
        for index in indices {
            let mut joint = Joint::default();
            animation_controller.get_joint(index, &mut joint);
            joint.m_Scale.data = [scale, scale, scale];
            animation_controller.set_joint(index, &mut joint);
        }
    }
}
