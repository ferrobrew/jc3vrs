//! Parachute locomotion detours: drop the camera-relative look-steer from the parachute's
//! steering while the head is decoupled, behind
//! [`suppress_parachute_look_steer`](crate::hooks::input::MovementConfig::suppress_parachute_look_steer).
//!
//! The parachute steering helper `NParachuteMovement::GetParachuteSteeringValues` mixes the
//! *camera input matrix* (`GameCameraManager::GetInputMatrix`) into the parachute's steering.
//! The camera-relative yaw/pitch deltas are shaped by the look-steer block
//! (`m_LookSteerDeadZone`, `m_LookSteerMaxYaw`, `m_LookSteerExponential`) into `look_steer_out`,
//! which is then mixed into the steering values and the yaw/pitch velocity springs. In VR the
//! camera follows the HMD, so turning the head past the deadzone turns the chute with no stick
//! input -- the "parachute yaws with the head" issue. The on-foot air-born suppression
//! ([`crate::hooks::input::locomotion`]) covers the jump/fall via `UpdateFallSteering`, but the
//! parachute is a separate movement family that never routes through that helper.
//!
//! The fix subtracts the look-steer contribution from `steering_values_out` (the direct
//! steering) and zeroes `look_steer_out` (which feeds the yaw/pitch velocity springs and the
//! slingshot blend) while the head is decoupled. The stick input
//! (`m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`) and the velocity-alignment
//! steering are untouched, so the chute still steers from the stick and banks with velocity.

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use jc3gi::{
    character::character::Character,
    input::parachute::CharacterParachuteSettings,
    types::math::{Vector2, Vector3},
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, hooks::input::locomotion::head_decoupled};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_PARACHUTE_STEERING_VALUES_BINDER)
}

/// How often the parachute look-steer was removed for the local player while the head was
/// decoupled. Read by the Game tab's movement readout.
pub static PARACHUTE_LOOK_STEER_SUPPRESSIONS: AtomicU64 = AtomicU64::new(0);

#[detour(address = jc3gi::input::parachute::GetParachuteSteeringValues_ADDRESS)]
fn get_parachute_steering_values(
    character: *mut Character,
    para_settings: *const CharacterParachuteSettings,
    input: *const Vector2,
    dt: f32,
    steering_values_out: *mut Vector3,
    wanted_char_yawpitchroll_out: *mut Vector3,
    look_steer_out: *mut Vector2,
) {
    GET_PARACHUTE_STEERING_VALUES.get().unwrap().call(
        character,
        para_settings,
        input,
        dt,
        steering_values_out,
        wanted_char_yawpitchroll_out,
        look_steer_out,
    );
    if should_suppress(character) {
        remove_look_steer(para_settings, steering_values_out, look_steer_out);
    }
}

/// Whether to remove the parachute look-steer this call: the toggle is on and the character is a
/// head-decoupled (VR) local player.
fn should_suppress(character: *mut Character) -> bool {
    if !Config::lock_query(|c| c.movement.suppress_parachute_look_steer) {
        return false;
    }
    unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter && head_decoupled(c))
}

/// Subtract the camera look-steer's contribution from the parachute steering values and zero the
/// look-steer output. The look-steer block mixed `m_XLookYawPitchRollAmount[axis] * look.x +
/// m_YLookYawPitchRollAmount[axis] * look.y` into each steering axis before the function
/// returned; removing exactly that leaves the stick input and the velocity alignment. `look_steer`
/// itself is read downstream by the caller (`UpdateParachutePhysics`) as the yaw/pitch velocity
/// spring inputs and the slingshot blend, so it is zeroed too.
fn remove_look_steer(
    para_settings: *const CharacterParachuteSettings,
    steering_values_out: *mut Vector3,
    look_steer_out: *mut Vector2,
) {
    let (Some(settings), Some(steering), Some(look)) = (
        unsafe { para_settings.as_ref() },
        unsafe { steering_values_out.as_mut() },
        unsafe { look_steer_out.as_mut() },
    ) else {
        return;
    };
    let air = &settings.m_AirControl;
    for axis in 0..3 {
        steering.data[axis] -= air.m_XLookYawPitchRollAmount[axis] * look.data[0]
            + air.m_YLookYawPitchRollAmount[axis] * look.data[1];
    }
    look.data = [0.0, 0.0];
    PARACHUTE_LOOK_STEER_SUPPRESSIONS.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with_look(x: [f32; 3], y: [f32; 3]) -> CharacterParachuteSettings {
        let mut settings = unsafe { std::mem::zeroed::<CharacterParachuteSettings>() };
        settings.m_AirControl.m_XLookYawPitchRollAmount = x;
        settings.m_AirControl.m_YLookYawPitchRollAmount = y;
        settings
    }

    /// A pure look-steer contribution is removed exactly from each steering axis, and the
    /// look-steer output is zeroed.
    #[test]
    fn removes_look_steer_from_steering() {
        let settings = settings_with_look([1.0, 0.5, 0.25], [0.1, 0.2, 0.3]);
        let mut steering = Vector3 {
            data: [10.0, 20.0, 30.0],
        };
        let mut look = Vector2 { data: [2.0, 4.0] };
        remove_look_steer(&raw const settings, &raw mut steering, &raw mut look);
        // axis 0: 10 - (1*2 + 0.1*4) = 10 - 2.4 = 7.6
        // axis 1: 20 - (0.5*2 + 0.2*4) = 20 - 1.8 = 18.2
        // axis 2: 30 - (0.25*2 + 0.3*4) = 30 - 1.7 = 28.3
        assert!((steering.data[0] - 7.6).abs() < 1e-5);
        assert!((steering.data[1] - 18.2).abs() < 1e-5);
        assert!((steering.data[2] - 28.3).abs() < 1e-5);
        assert!(look.data[0] == 0.0 && look.data[1] == 0.0);
    }

    /// With zero look-steer the steering is unchanged and the output stays zero.
    #[test]
    fn zero_look_steer_leaves_steering() {
        let settings = settings_with_look([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mut steering = Vector3 {
            data: [5.0, 6.0, 7.0],
        };
        let mut look = Vector2 { data: [0.0, 0.0] };
        remove_look_steer(&raw const settings, &raw mut steering, &raw mut look);
        assert!((steering.data[0] - 5.0).abs() < 1e-6);
        assert!((steering.data[1] - 6.0).abs() < 1e-6);
        assert!((steering.data[2] - 7.0).abs() < 1e-6);
    }
}
