# VR controller input

Driving the head/body from the game's own look and move input, and — eventually — from VR controllers so it isn't gamepad-only in the headset. The read side (tapping and consuming the engine's input so our schemes own the look and move deltas) ships today; the write side (injecting VR controller state as native input) is not yet wired.

## What's known

The engine exposes input through action effectors: `LocalPlayerActionMap::GetActionEffector` returns an `SEffectorElement` per action, and there is a `Freeze` path. The camera look axes are `LOOK_LEFT` / `LOOK_RIGHT` / `LOOK_UP` / `LOOK_DOWN` (net `yaw = right − left`, `pitch = up − down`), and the move axes are `MOVE_LEFT` / `MOVE_RIGHT` / `MOVE_FORWARD` / `MOVE_BACKWARD` (IDs 28–31). All of these resolve through the `Action` enum (see below). The gameplay buttons (fire, jump, grapple, reload, enter-vehicle, …) are likewise enumerated, but only look and move are consumed today.

## Tapping the look input

For VR (and the flatscreen prototype) we drive the head/body ourselves, so the game's own look input must stop rotating the camera and feed our scheme instead. We tap the engine's device poll rather than the camera pipeline.

The shipped detour is on `InputDeviceManager::Update(dt)` — the per-frame device poll (`payload/src/hooks/input/look.rs`). After the original poll runs, the detour reads the four look effectors off `LocalPlayerActionMap`, clears them (zeros `m_Value` / `m_PrevValue`, sets `m_State` to `Idle`) so the game's camera system sees no look input, and feeds the net deltas into the headpose simulation via `headpose::sim::on_input_tick`. Running post-poll means the values are the fully-resolved per-frame deltas; clearing them at the effector is safe because the look effectors feed camera code only, so nulling them doesn't disturb aim assist (which derives from the resulting camera orientation). When egui captures input the detour still ticks the sim with a zero delta, so the latch and mode-detection cadence keep running.

The tick runs on the engine's fixed-rate sim tick, inside the `Update` hook, so the published pose pair rotates in phase with the engine's sub-frame interpolation reset — see `payload/src/headpose/sim.rs`.

**Why not the camera pipeline.** An earlier approach considered tapping the look path where the camera consumes it: `GameCameraManager::UpdateBlackboardValues` reads gamepad look, and `InputToOrbitModifier::CalculateInputDeltaAngles` combines the devices, applies per-axis sensitivity, and returns the final delta-angle, which its caller `InputToOrbitModifier::ProcessCameraContext` applies via `BoomTransform::DeltaTransform`. `CalculateInputDeltaAngles` carries an anti-tamper codemarker — **do not hook it or anything inside it.** `ProcessCameraContext` itself is clean, but consuming the effectors post-poll is simpler and keeps us out of the codemarker's blast radius entirely, so that is what shipped.

## Tapping the move input

Move is handled in the locomotion tasks rather than by clearing effectors (`payload/src/hooks/input/locomotion.rs`). The always-on FPS-movement shim (`MovementConfig::force_fps_movement`) forces the local player's aim flags (`Character::m_AimFlags`, `m_AimingWeapon`) only for the duration of each `NStateTask_InputLocoMoveTask` / `NStateTask_InputLocoAimRelativeTask` `Update`, and restores them immediately after. The locomotion tasks pick between run/steer and aim-relative strafe by reading those flags — the same flags that drive the weapon raise, the reticle, and auto-aim — so forcing them only across the task's `Update` makes the movement branch see "aiming" (directional strafe) while the aim system never does. Forcing the flags globally would drag the weapon raise and reticle in with it; the scoped force avoids that. The character updates on its own worker thread, so nothing else reads the flags mid-task.

`NStateTask_InputLocoSetTargetDirTask::SetupTargetDir` reads the move axes, rotates them camera-relative, and writes the move vectors to the character blackboard; the headpose body-yaw scheme and the slide overrides build on top of it (heading via the target-face-dir blackboard, displacement via `EvaluateCharacterDisplacement`). See `head-and-body.md` for the head/body coupling.

## The action IDs

The action-name-to-ID table (`action_name_table` at `0x142D99370`) is a fixed array of 255 name pointers indexed by ID, built once at startup — the numbering is byte-stable across builds, so the IDs are **hardcoded** rather than discovered live. All 255 are captured as the `Action` enum (`input/input_action_map.pyxis`), from `PAUSE = 0` through `GUI_USE_BUTTON = 0xFE`, and the effector accessors and the (future) write API take it directly. (Runtime key/axis bindings load separately via `LoadActionMapFromFile`, but the action-ID numbering itself is static.) Live logging inside `GetActionEffector` was the original discovery route and is now moot since the enum exists.

## The write path (future)

Not yet implemented. The intended design injects VR controller state as effector values so the engine reads VR input as native input — the sticks and triggers as effector axes, the buttons as effector states — gated so real-gamepad input and VR input don't fight. Locomotion direction and turning would tie into the head/body scheme (`head-and-body.md`).

The write API is on `LocalPlayerActionMap` (its instance pointer is a global; the def models it as a singleton). Drive an action by ID: `ForceSetPressed(actionId, value)` for analog/held inputs (sticks, triggers, held buttons), and `ForceSetClicked(actionId)` for a one-frame press edge. Both guard against the null-effector sentinel that invalid IDs resolve to. The low-level effector mutators (`Click`, `Press`, `Freeze`, `ForceClick`) are also exposed on `InputDeviceEffector` if a specific effector needs driving directly.

**Timing.** Inject *after* `InputDeviceManager::Update(dt)` — the per-frame device poll — or the poll overwrites whatever you wrote. `Update` runs `UpdateForceClicks` after polling, which is the `m_ForceClick` mechanism: a click latched via `ForceSetClicked`/`ForceClick` sets the effector's `m_ForceClick` flag (`InputDeviceEffector+0xF`) so the press edge survives that poll instead of being cleared by it. So the robust pattern is to set held/analog values right after `Update`, and to use the force-click path for momentary buttons so they aren't lost to poll timing. (`InputDeviceManager::m_Enabled` gates the whole poll and doubles as a freeze handle.)
