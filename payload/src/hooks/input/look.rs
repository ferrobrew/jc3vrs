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

use crate::headpose;

#[detour(address = jc3gi::input::input_device_manager::InputDeviceManager::Update_ADDRESS)]
pub(super) fn input_device_manager_update(manager: *mut InputDeviceManager, dt: f32) {
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
        headpose::sim::on_input_tick(0.0, 0.0);
        return;
    }

    let Some(map) = (unsafe { LocalPlayerActionMap::get() }) else {
        return;
    };
    unsafe {
        let look_x = read_look_axis(map, Action::LOOK_RIGHT, Action::LOOK_LEFT);
        let look_y = read_look_axis(map, Action::LOOK_UP, Action::LOOK_DOWN);

        clear_effector(map, Action::LOOK_LEFT);
        clear_effector(map, Action::LOOK_RIGHT);
        clear_effector(map, Action::LOOK_UP);
        clear_effector(map, Action::LOOK_DOWN);

        headpose::sim::on_input_tick(look_x, look_y);
    }
}

/// Read two opposite-direction effectors and return the net delta.
unsafe fn read_look_axis(
    map: &mut LocalPlayerActionMap,
    positive: Action,
    negative: Action,
) -> f32 {
    let pos = unsafe { read_effector_value(map, positive) };
    let neg = unsafe { read_effector_value(map, negative) };
    pos - neg
}

/// Read an effector's analog value safely.
unsafe fn read_effector_value(map: &mut LocalPlayerActionMap, action: Action) -> f32 {
    unsafe {
        map.GetActionEffector(action)
            .as_ref()
            .map(|effector| effector.m_Value)
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
