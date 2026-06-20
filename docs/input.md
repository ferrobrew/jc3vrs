# VR controller input

Driving the game from VR controllers so it isn't gamepad-only in the headset. The read side of the engine's input is already reversed; the missing piece is mapping controller state onto the engine's action effectors, which needs the per-action IDs discovered.

## What's known

The engine exposes input through action effectors: `GetActionEffector` (`0x1402F43B0`, from PLAN §15.9 — re-verify the release address) returns an `SEffectorElement`, and there is a `Freeze` path. The input definitions exist. But of the 255 action IDs, only the right-stick IDs 1–4 are currently known — every gameplay button (fire, jump, grapple, reload, enter-vehicle, and so on) needs its ID discovered before a VR controller can drive it.

## Discovering the action IDs

The IDs aren't in a header; observe them live. Log inside `GetActionEffector` (`0x1402F43B0`) — record the `(action_id, device_index)` each time the game reads an effector, press each button in turn, and build the table. Alternatively, hunt for the action-map asset the §15.9 brief suspects exists (an XML or binary map) and read the IDs from it.

## The write path

Once the IDs are known, inject VR controller state as effector values so the engine reads VR input as native input — drive the sticks and triggers as effector axes and the buttons as effector states, gated so real-gamepad input and VR input don't fight. Locomotion direction and turning tie into the head/body scheme (`docs/head-and-body.md`).
