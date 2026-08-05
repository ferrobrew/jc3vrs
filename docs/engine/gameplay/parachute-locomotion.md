# Parachute locomotion and steering

How the game moves and steers a parachuting character. Established from the 2016 symbol dump (real symbol names) and the release IDB / name table (2026-NoDenuvo `combined_names.json`), cross-checked against the 2016 MAP+PDB release decompile. The load-bearing finding: **the parachute steering helper mixes the *camera input matrix* into the parachute's steering** — the camera-relative yaw/pitch look-steer block turns the chute toward where the camera points, not only where the stick points.

## The state task

Parachuting runs on its own movement state task that delegates to a free-function steering core:

| Function | Release address |
|---|---|
| `NStateTask_MovementParachuteTask::Update` | `0x14082AB10` |
| `NParachuteMovement::UpdateParachutePhysics` | `0x1407E6DF0` |
| `NParachuteMovement::GetParachuteSteeringValues` | `0x1407E6480` |
| `NParachuteMovement::UpdateParachutingTransforms` | `0x140792880` |
| `NParachuteMovement::ApplyConventionalParachuteVelocities` | `0x140792D10` |
| `NParachuteMovement::ApplyAnimationVelocityWithSteering` | `0x1407751E0` |

`NStateTask_MovementParachuteTask::Update` reads the parachute movement effectors (pitch/yaw inputs via the action map, indices 28–31), calls `UpdateParachutePhysics` with the resulting `CVector2f` input, then handles the cancel input that closes the parachute.

## The steering helper: `GetParachuteSteeringValues`

`UpdateParachutePhysics` calls `GetParachuteSteeringValues` (`0x1407E6480`) each frame. It computes three outputs:

- `steering_values_out` — the yaw/pitch/roll steering mix for the frame;
- `wanted_char_yawpitchroll_out` — the desired character orientation;
- `look_steer_out` — the camera-relative look-steer vector.

The steering is the sum of two independent mixes:

**Stick input.** `input->x`/`input->y` through `m_XInputYawPitchRollAmount`/`m_YInputYawPitchRollAmount`: the plain controller steering, present even with no camera.

**Camera look-steer.** The function reads the camera input matrix (`CGameCameraManager::GetInputMatrix`, `0x140_75C_7A0`), decomposes it to Euler angles, and computes the yaw/pitch deltas between the camera and the character's current wanted orientation (`m_SteeringProxy`). A deadzone/max-yaw/exponential shaping block (`m_LookSteerDeadZone`, `m_LookSteerMaxYaw`, `m_LookSteerExponential`) converts those deltas into `look_steer_out`, which is then:

- mixed into `steering_values_out` on all three axes via `m_XLookYawPitchRollAmount`/`m_YLookYawPitchRollAmount` (the `v63/v61/v62` sum at the tail of the helper);
- consumed by `UpdateParachutePhysics` as the yaw/pitch velocity spring inputs (`v254.x` → `m_RotateCharYawToVelocity`, `v254.y` → `m_RotateCharPitchToVelocity`) and in the slingshot/animation-input blend (`1.0 - look_steer_out.x`).

So the parachute's heading follows the camera direction once the camera-relative yaw exceeds the look-steer deadzone — with or without stick input. The look-steer block only runs for the local player (`character->m_IsPlayer`); NPC parachutes steer from the stick mix alone.

## Orientation application

`UpdateParachutePhysics` applies the heading through the parachute action params' proxy matrices (`SParachuteActionParams` in `CCharacter`): `m_PivoProxy` (the suspension pivot), `m_CharacterProxy`, `m_ParachuteProxy`, and the blended `m_SteeringProxy` that interpolates between them. `UpdateParachutingTransforms` (`0x140792880`) copies the steering matrix into `m_SteeringProxy`, converts it to a quaternion, and drives the parachute rig's swing; `ApplyConventionalParachuteVelocities` (`0x140792D10`) and `ApplyAnimationVelocityWithSteering` (`0x1407751E0`) compute the velocity response. The character's world orientation follows the `m_SteeringProxy` rotation.

## The velocity-alignment term

Alongside the stick and camera mixes, `GetParachuteSteeringValues` computes a velocity-alignment yaw: when the horizontal velocity exceeds `m_VelocityAlignMinSpeed`, the wanted yaw is driven toward the velocity heading through `m_RotateCharYawToVelocity` (a `SCharacterSpring`), blended by `m_VelocitySteeringBlendScale`. This is what keeps the canopy squared to the travel direction when not steering.

## Related

- The airborne/fall family that *does* route through the on-foot-style helper: `UpdateFallSteering` (`0x1407916F0`) and `UpdateStdLookSteering` — documented in the pyxis defs (`input/locomotion.pyxis`). The parachute does **not** use those; it is a separate movement core.
- The shared air tuning block `SCharacterAirMovementSettings` (the `m_LookSteer*`/`m_XInputYawPitchRollAmount`/spring fields) is also embedded in the wingsuit and freefall settings.
- Pyxis defs for the parachute family: `input/parachute.pyxis`.