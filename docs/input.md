# VR controller input

Driving the game from VR controllers so it isn't gamepad-only in the headset. The read side of the engine's input is already reversed; the missing piece is mapping controller state onto the engine's action effectors, which needs the per-action IDs discovered.

## What's known

The engine exposes input through action effectors: `GetActionEffector` (`0x1402F43B0`) returns an `SEffectorElement` per device — `GetDevice(mgr, deviceType, …)` with deviceType 2 = mouse, 4 = gamepad — and there is a `Freeze` path. Of the 255 action IDs, the camera look axes are known (`yaw = eff(3) − eff(4)`, `pitch = eff(1) − eff(2)`) and the move axes are IDs 28–31 (`moveY = eff(28) − eff(29)`, `moveX = eff(31) − eff(30)`, read via the action-map's own `GetEffector` at vtable+72, not the `0x1402F43B0` wrapper). The gameplay buttons (fire, jump, grapple, reload, enter-vehicle, …) still need their IDs discovered before a VR controller can drive them.

## Tapping the pipeline for VR camera and movement

For VR (and the flatscreen prototype) we drive the head/body ourselves, so the game's own look input must stop rotating the camera, and the look/move input should feed our scheme instead. Tap the game's native feeders rather than reading devices ourselves.

The look path is: `CGameCameraManager::UpdateBlackboardValues` (`0x1407FFF90`) reads gamepad look and writes blackboard key `3560844826`; `CInputToOrbitModifier::CalculateInputDeltaAngles` (`0x1406CB3F0`) reads mouse look, combines the devices, applies per-axis sensitivity, and returns the final delta-angle. **That last function carries a Denuvo anti-tamper codemarker (`m_InputScale *= -1`) — do not hook it or anything inside it.** Its caller `CInputToOrbitModifier::ProcessCameraContext` (`0x1406DBB80`) is clean, and is where the delta-angle is applied to the camera via `CBoomTransform::DeltaTransform` (`0x14043D180`).

So tap `ProcessCameraContext`: after `CalculateInputDeltaAngles` returns, read the combined delta (both devices, post-sensitivity) for our scheme, then zero it before `DeltaTransform` applies it. Nulling at the *apply* site — not by zeroing the effectors — keeps aim working: aim assist derives from the resulting camera orientation, and the look effectors (IDs 1–4) feed camera code only, confirmed across all 161 `GetActionEffector` xrefs.

Move is separate: `SetupTargetDir` (`0x14081E130`) reads the move axes (IDs 28–31), rotates them camera-relative, and writes blackboard keys `923417185` (raw input) and `2113030792` (camera-relative world dir). Read the **raw** vector (or key `923417185`) so we can supply our own VR-body-relative basis. The grapple/stunt variants share the IDs (`0x140820CD0`, `0x140819DE0`). All these tap sites are codemarker-free; confirm the exact stack layout at the `DeltaTransform` call (`0x1406DBCFD`) before placing the null.

## Discovering the action IDs

The IDs aren't in a header; observe them live. Log inside `GetActionEffector` (`0x1402F43B0`) — record the `(action_id, device_index)` each time the game reads an effector, press each button in turn, and build the table. Alternatively, hunt for the action-map asset the §15.9 brief suspects exists (an XML or binary map) and read the IDs from it.

## The write path

Once the IDs are known, inject VR controller state as effector values so the engine reads VR input as native input — drive the sticks and triggers as effector axes and the buttons as effector states, gated so real-gamepad input and VR input don't fight. Locomotion direction and turning tie into the head/body scheme (`docs/head-and-body.md`).
