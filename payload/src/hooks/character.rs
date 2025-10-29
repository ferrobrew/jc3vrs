use jc3gi::character::character::{Character, get_Character_GoreEnabled};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        // Enable "gore" (not actually used anywhere!) so that we can
        // disable the player's head
        .with_patch(
            unsafe { get_Character_GoreEnabled() } as *const bool as usize,
            &[1],
        )
        // Change the popped-head check to check for whether the character
        // is a local character instead
        .with_patch(
            0x143_AC2_3BE,
            &(std::mem::offset_of!(Character, m_IsLocalCharacter) as u32).to_le_bytes(),
        )
}
