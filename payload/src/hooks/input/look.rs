//! The `InputDeviceManager::Update` detour: read and consume the game's own look effectors so the
//! headpose simulation owns the mouse-look deltas.
//!
//! The game polls devices via `InputDeviceManager::Update(dt)`, which populates `InputDeviceEffector`
//! slots including `LOOK_LEFT` / `LOOK_RIGHT` / `LOOK_UP` / `LOOK_DOWN`. These are the mouse-look
//! deltas the game's camera system reads. We intercept them post-call: read the values, clear the
//! effectors so the game's camera doesn't respond, and feed the deltas into the headpose simulation.

use detours_macro::detour;
use jc3gi::input::{
    input_action_map::{Action, EffectorState, LocalPlayerActionMap},
    input_device_manager::InputDeviceManager,
};

use re_utilities::hook_library::HookLibrary;

use crate::headpose;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&INPUT_DEVICE_MANAGER_UPDATE_BINDER)
}

#[detour(address = jc3gi::input::input_device_manager::InputDeviceManager::Update_ADDRESS)]
fn input_device_manager_update(manager: *mut InputDeviceManager, dt: f32) {
    INPUT_DEVICE_MANAGER_UPDATE.get().unwrap().call(manager, dt);

    if !headpose::is_active() {
        return;
    }
    // When egui captures input, publish a zero-delta tick — the game's input is already disabled,
    // but the sim's tick cadence (mode detection, pose-pair rotation) must keep running.
    if crate::egui_impl::EguiState::get()
        .as_ref()
        .is_some_and(|s| s.is_input_captured())
    {
        headpose::sim::on_input_tick(0.0, 0.0, false, dt);
        return;
    }

    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return;
    };
    unsafe {
        let (look_x, look_x_delta) = read_look_axis(map, Action::LOOK_RIGHT, Action::LOOK_LEFT);
        let (look_y, _) = read_look_axis(map, Action::LOOK_UP, Action::LOOK_DOWN);

        clear_effector(map, Action::LOOK_LEFT);
        clear_effector(map, Action::LOOK_RIGHT);
        clear_effector(map, Action::LOOK_UP);
        clear_effector(map, Action::LOOK_DOWN);

        headpose::sim::on_input_tick(look_x, look_y, look_x_delta, dt);
    }
}

/// Read two opposite-direction effectors and return the net value plus whether it is delta-based (a
/// mouse per-tick displacement) rather than an absolute axis (a stick position). Delta-based if either
/// contributing effector is non-zero and delta-based -- the two device kinds must be turned into body
/// yaw differently (see [`headpose::xr::advance_body_yaw`]).
unsafe fn read_look_axis(
    map: &mut LocalPlayerActionMap,
    positive: Action,
    negative: Action,
) -> (f32, bool) {
    let (pos, pos_delta) = unsafe { read_effector_value(map, positive) };
    let (neg, neg_delta) = unsafe { read_effector_value(map, negative) };
    let delta_based = (pos != 0.0 && pos_delta) || (neg != 0.0 && neg_delta);
    (pos - neg, delta_based)
}

/// Read an effector's analog value and its delta-based flag safely (`(0.0, false)` when absent).
unsafe fn read_effector_value(map: &mut LocalPlayerActionMap, action: Action) -> (f32, bool) {
    unsafe {
        map.GetActionEffector(action)
            .as_ref()
            .map(|effector| (effector.m_Value, effector.m_IsDeltaBased))
            .unwrap_or_default()
    }
}

/// Zero the effector's `m_Value` and set `m_State` to `Idle`, so the game's camera system sees no
/// input.
unsafe fn clear_effector(map: &mut LocalPlayerActionMap, action: Action) {
    let effector_ptr = unsafe { map.GetActionEffector(action) };
    if let Some(effector) = unsafe { effector_ptr.as_mut() } {
        effector.m_Value = 0.0;
        effector.m_PrevValue = 0.0;
        effector.m_State = EffectorState::Idle;
    }
}
