//! Animation hooks: freeze Rico's idle breathing.
//!
//! While the local player stands in the base idle state (`S_IDLE`), the idle clip keeps advancing,
//! producing the subtle breathing/sway. With the head driven by the HMD and the body meant to hold
//! the player's pose, that motion reads as the body drifting on its own (issue #33). These hooks zero
//! the animation-clock advance for the local player's controller while it is in `S_IDLE`, holding the
//! pose static; every other state (and every other character) runs at normal speed. The periodic
//! idle *fidget* is handled separately in `crate::hooks::input::locomotion`.

use std::sync::LazyLock;

use detours_macro::detour;
use jc3gi::{
    character::character::{AnimationController, Character},
    hash::hashlittle,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

/// The type id (`hashlittle("S_IDLE")`, the engine's own `HashString`) of the on-foot base idle rule
/// state, resolved via the game's own hash so it stays correct rather than hard-coding the value.
static S_IDLE_TYPE_ID: LazyLock<u32> = LazyLock::new(|| hashlittle(b"S_IDLE") as u32);

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ANIMATIONS_TIME_BINDER)
        .with_static_binder(&UPDATE_ANIMATIONS_BINDER)
}

/// Advance the controller's animation clocks -- but hold them (`dt = 0`) while the local player is in
/// the base idle state, freezing the idle breathing.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimationsTime_ADDRESS
)]
fn update_animations_time(controller: *mut AnimationController, dt: f32) {
    let dt = if unsafe { should_freeze(controller) } {
        0.0
    } else {
        dt
    };
    UPDATE_ANIMATIONS_TIME.get().unwrap().call(controller, dt);
}

/// Recompute the controller's pose -- with `dt = 0` while the local player is in the base idle state,
/// so the pose is resampled at a held clock (`CPoseProducer::Update` also advances the clock) instead
/// of drifting.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimations_ADDRESS
)]
fn update_animations(controller: *mut AnimationController, dt: f32, num_of_bones: i32) {
    let dt = if unsafe { should_freeze(controller) } {
        0.0
    } else {
        dt
    };
    UPDATE_ANIMATIONS
        .get()
        .unwrap()
        .call(controller, dt, num_of_bones);
}

/// Whether this controller belongs to the local player, that player is standing in `S_IDLE`, and the
/// breathing suppression is enabled. The controller identity is checked first so a non-player update
/// costs only a pointer compare (no config lock, no state walk).
unsafe fn should_freeze(controller: *mut AnimationController) -> bool {
    unsafe {
        let Some(player) = Character::GetLocalPlayerCharacter().as_ref() else {
            return false;
        };
        if !std::ptr::eq(
            player.m_AnimatedModel.m_AnimationController as *const AnimationController,
            controller,
        ) {
            return false;
        }
        Config::lock_query(|c| c.movement.suppress_idle_breathing)
            && active_state_type_id(player) == Some(*S_IDLE_TYPE_ID)
    }
}

/// The character's active animation rule-state id (`hashlittle(state_name)`, e.g. `S_IDLE`), read
/// through its first (body) rule system's state machine, mirroring `CCharacter::IsInVehicleAttachState`.
/// `None` if any link in the chain is absent (loading, no rule system).
unsafe fn active_state_type_id(character: &Character) -> Option<u32> {
    unsafe {
        Some(
            character
                .m_AnimatedModel
                .m_RuleSystems
                .as_slice()
                .first()?
                .as_ref()?
                .m_StateMachineInstance
                .as_ref()?
                .m_CurrentState
                .as_ref()?
                .m_HashID
                .m_Hash,
        )
    }
}
