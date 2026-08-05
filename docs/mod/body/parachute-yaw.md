# Parachute head-yaw suppression

Why the parachute turned with the head in VR, and how the mod stops it. The engine-side mechanism is [engine/gameplay/parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md); the implementation is `payload/src/hooks/input/parachute.rs` behind `MovementConfig::suppress_parachute_look_steer` (default on).

## The problem

The parachute steering helper `NParachuteMovement::GetParachuteSteeringValues` mixes the **camera input matrix** into the chute's steering. In VR the camera follows the HMD, so turning the head past the look-steer deadzone (`m_LookSteerDeadZone`, shaped by `m_LookSteerMaxYaw`/`m_LookSteerExponential`) turns the parachute — with **no stick input at all**. The whole body frame rotates under the player, since the view is composed on the body frame (`body × cockpit`).

The mod's existing airborne suppression (`movement.suppress_air_aim_facing`) could not reach it: that hooks `UpdateFallSteering`, which governs the jump ascent and the fall, but the parachute is a separate movement core that never routes through that helper (see [engine/gameplay/parachute-locomotion.md](../../engine/gameplay/parachute-locomotion.md)).

## The fix

A single detour on `GetParachuteSteeringValues`. After the original runs, while the head is decoupled (VR) and the toggle is on, the look-steer contribution is removed:

- **`steering_values_out`** — the camera look-steer had mixed `m_XLookYawPitchRollAmount[axis]·look.x + m_YLookYawPitchRollAmount[axis]·look.y` into each axis; that exact amount is subtracted, leaving the stick input and the velocity-alignment intact.
- **`look_steer_out`** — zeroed. The caller (`UpdateParachutePhysics`) otherwise feeds this into the yaw/pitch velocity springs (`m_RotateCharYawToVelocity`/`m_RotateCharPitchToVelocity`) and the slingshot blend, so leaving it would still let the head steer the springs even with the steering values cleaned.

Gated on the same `head_decoupled` predicate as the other VR body-turn suppressions: under the VR source it is always true for the local player, and on flatscreen it only applies in the decoupled-idle-not-aiming window. NPC parachutes are untouched (the game's look-steer block already skips non-players, and the hook only rewrites the local player's outputs).

## Validation

The Game tab's movement section shows `Parachute: look-steer suppressions`. In-headset checks: deploy the chute, hold the stick at rest, and turn the head past 90° — the chute should fly straight; the stick should still steer; and the velocity banking and the drift-toward-velocity alignment should still work. The suppression counter advances while the chute is deployed in VR and its output is rewritten; it stays put on flat ground.