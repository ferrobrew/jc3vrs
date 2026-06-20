# VR controller input

Driving the game from VR controllers so it isn't gamepad-only in the headset. The read side of the engine's input is already reversed; the missing piece is mapping controller state onto the engine's action effectors, which needs the per-action IDs discovered.

## What's known

The engine exposes input through action effectors: `GetActionEffector` returns an `SEffectorElement` per device (`GetDevice` with deviceType 2 = mouse, 4 = gamepad), and there is a `Freeze` path. Of the 255 action IDs, the camera look axes are known (`yaw = eff(3) − eff(4)`, `pitch = eff(1) − eff(2)`) and the move axes are IDs 28–31 (`moveY = eff(28) − eff(29)`, `moveX = eff(31) − eff(30)`, read via the action-map's own `GetEffector` at vtable+72, not the `GetActionEffector` wrapper). The gameplay buttons (fire, jump, grapple, reload, enter-vehicle, …) still need their IDs discovered before a VR controller can drive them.

## Tapping the pipeline for VR camera and movement

For VR (and the flatscreen prototype) we drive the head/body ourselves, so the game's own look input must stop rotating the camera, and the look/move input should feed our scheme instead. Tap the game's native feeders rather than reading devices ourselves.

The look path is: `GameCameraManager::UpdateBlackboardValues` reads gamepad look into a camera blackboard key; `InputToOrbitModifier::CalculateInputDeltaAngles` reads mouse look, combines the devices, applies per-axis sensitivity, and returns the final delta-angle. **That last function carries an anti-tamper codemarker — do not hook it or anything inside it.** Its caller `InputToOrbitModifier::ProcessCameraContext` is clean, and is where the delta-angle is applied to the camera via `BoomTransform::DeltaTransform`.

So tap `ProcessCameraContext`: after `CalculateInputDeltaAngles` returns, read the combined delta (both devices, post-sensitivity) for our scheme, then zero it before `DeltaTransform` applies it. Nulling at the *apply* site — not by zeroing the effectors — keeps aim working: aim assist derives from the resulting camera orientation, and the look effectors (IDs 1–4) feed camera code only, confirmed across all 161 `GetActionEffector` xrefs.

Move is separate: `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir` reads the move axes (IDs 28–31), rotates them camera-relative, and writes the raw and camera-relative move vectors to the character blackboard. Read the **raw** vector (before the rotation) so we can supply our own VR-body-relative basis. The stunt and reeled-in variants read the same IDs. All these tap sites are codemarker-free; confirm the exact stack layout at the `DeltaTransform` call (`ProcessCameraContext+0x17D`) before placing the null.

## Discovering the action IDs

The IDs aren't in a header; observe them live. Log inside `GetActionEffector` — record the `(action_id, device_index)` each time the game reads an effector, press each button in turn, and build the table. Alternatively, hunt for the action-map asset (an XML or binary map) that's suspected to exist and read the IDs from it.

## The write path

Once the IDs are known, inject VR controller state as effector values so the engine reads VR input as native input — drive the sticks and triggers as effector axes and the buttons as effector states, gated so real-gamepad input and VR input don't fight. Locomotion direction and turning tie into the head/body scheme (`head-and-body.md`).
