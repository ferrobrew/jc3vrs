# Swim locomotion

How the game moves and turns a swimming character. Established from the 2016 symbol dump and verified against the release IDB (2026-NoDenuvo); the release addresses and the act-dispatch structure match. The load-bearing finding: **the swim family is a separate locomotion pipeline that never touches the on-foot orientation machinery** — no `NStateTask_LocoUtil::EvaluateCharacterOrientation`, no target-face-dir blackboard, no `GetAimMoveAngle` — and its body yaw changes in act-sized steps rather than continuously.

## The state tasks

Swimming runs on its own input/movement state-task pairs, one pair per stratum:

| Task | Release address |
|---|---|
| `NStateTask_InputSurfaceSwimTask::Update` | `0x140830BA0` |
| `NStateTask_MovementSurfaceSwimTask::Update` | `0x14082C940` |
| `NStateTask_InputUnderWaterSwimTask::Update` | `0x140823FC0` |
| `NStateTask_MovementUnderWaterSwimTask::Update` | `0x14082D470` |
| `UpdateSurfaceMovement` (player movement core) | `0x1407E9440` |
| `UpdateUnderwaterMovement` (movement core) | `0x1407A5DC0` |
| `UpdateSurfaceMovementNPC` | `0x1407A5710` |

The movement tasks delegate to the free-function cores; the surface task branches player/NPC, so `UpdateSurfaceMovement` is player-only.

## Turn dispatch: discrete acts by angle threshold

The input tasks compute a desired direction and measure the signed XZ angle from the body's current forward with `CControllerUtility::GetDeltaAngleFromOrientation` (`0x140781410`). The desired direction is:

- **moving**: the camera-relative move direction (stick through `GameCameraManager::GetInputMatrix`);
- **idle while wielding a weapon** (surface): the direction to the player's aim-target position;
- **grapple/planted-explosive**: the direction to the grapple or explosive target.

The angle picks an act, not a turn rate:

- |Δ| < 65° → the forward crawl-start act (`ACT_SURFACE_SWIM_START_0`).
- 65° ≤ |Δ| ≤ 180° → a **120° turn clip**: `ACT_SURFACE_SWIM_START_120R/L` while moving, `ACT_SURFACE_IDLE_QUICKTURN_120R/L` for the armed idle, `ACT_UW_SWIM_IDLE_TURN_120R/L` underwater.

When a turn act is latched, the task writes the angle to the blackboard (float id `0xA894C92D` / `2828323117`) and stores the current forward and the desired direction in the character's swim action parameters (`m_AnimatedTurnBaseDir` / `m_AnimatedTurnTargetDir`).

## Orientation application

The movement cores own the actual heading. Each update, `UpdateSurfaceMovement` produces a desired forward one of two ways, and they are mutually exclusive:

1. **Animated turn (act-quantized).** If the **angle-correction animation segment** is playing, the heading is `SlerpAroundAxis(m_AnimatedTurnBaseDir, m_AnimatedTurnTargetDir, t)` (`0x140775620`) at the segment's local time `t` — the latched turn plays out across the clip window, then stops. This path short-circuits past everything below.
2. **Continuous rotation.** Otherwise the core rotates the character's current forward toward a desired direction with `DoRotate` (`0x1407754F0`, called at `0x1407EA347`), limited to `swim_settings[0x94] * 30 * dt` degrees, which a capture put at ~69°/s (the multiplier lives in a `.bin` resource, so that figure is measured rather than read). The aim and planting-explosive branch overrides the rate to a flat 300°/s (`0x1407EA1C4`). The desired direction is the world move direction from `TransformInputDirToWorldDir` while moving, the aim-target direction while aiming (at a fixed 300°/s), the grapple target while reeling, or — with the stick neutral — **the character's own current forward**, so a swimmer with no stick input never rotates.

The resulting forward is blended with the water-surface plane for roll (`UpdateSurfacePlane`, `0x1407937C0`) and assembled with `CMatrix4f::CreateOrientation` (`0x14002E3F0`, called at `0x1407EA49E`).

`UpdateUnderwaterMovement` has the same two paths but differs in three ways that matter to any consumer substituting the direction:

- It **does not call `TransformInputDirToWorldDir`**. It builds the move direction inline, rotating the local vector `(forward, 0, -lateral)` by the camera's full input matrix — so the camera's *pitch* is baked in, and looking up or down while pushing forward is how the swimmer ascends and descends.
- That direction is three-dimensional throughout, and `DoRotate`'s output becomes both the facing and (scaled) the velocity, so **the swimmer moves where it faces**.
- Its rotation rate is a fixed `30 * dt` degrees, slower still than the surface's.

The consequence: while a turn act runs the heading is quantized to the clip window, and outside one it only tracks the camera-relative stick. A third-person camera hides both; anything that composes a view on the body frame sees every step.

## The capsule offset: `SetOrientation` does not receive the facing frame

Before the orientation reaches the physics body it is composed with a fixed **swimming capsule offset**, chosen per frame by speed (`0x1407EA78F`):

| Pose | Offset | Value |
|---|---|---|
| Surface, wanted horizontal speed ≤ 0.1 | `m_SurfaceIdleSwimmingCapsuleOffset` | identity |
| Surface, moving | `m_SurfaceCrawlSwimmingCapsuleOffset` | `RotationX(-1.5707951)` — exactly −π/2 |
| Underwater | `m_UwSwimmingCapsuleOffset` | `RotationX(-1.256636)` (≈ −72°) plus a small downward translation |

All three are built in `SSwimActionParams::SSwimActionParams` (`0x140779BF0`). The core composes the selected offset onto its orientation, orthonormalizes, copies the matching inverse to `m_CurrentSwimmingCapsuleOffsetInv`, and only then calls `CPfxCharacterInstance::SetOrientation` (`0x140239100`), which sets the physics body's rotation from the matrix and zeroes its angular velocity.

So the matrix handed to `SetOrientation` is the **capsule frame**, not the facing frame. For the surface crawl and underwater poses it is pitched steeply — the crawl case sits exactly at gimbal lock for a yaw/pitch/roll decomposition — and recovering a heading from it requires `m_CurrentSwimmingCapsuleOffsetInv` first.

## Related

- The on-foot orientation pipeline this family bypasses: `NStateTask_LocoUtil::EvaluateCharacterOrientation` (`0x14081F8C0`), documented in the pyxis defs (`input/locomotion.pyxis`).
- Pyxis defs for the swim family: `input/swim.pyxis`, `input/controller_utility.pyxis`, `physics.pyxis`, `character/swim_action_params.pyxis`, and the blackboard id in `blackboard.pyxis`.
- `DoRotate` has three call sites in the binary: the surface heading rotation above, the underwater one, and the velocity slerp inside `ProcessMotion` (`0x140775760`, called at `0x1407E9AFC`). It scales its step by `min(angle / 15°, 1) * rate` and does **not** clamp to the remaining angle, so a rate above 15° per call overshoots and above 30° diverges.
- `TransformInputDirToWorldDir` (`0x140782430`) is a general input helper, not a swim one: fifteen call sites spanning locomotion target-dir, jump, melee, grapple hang and reel-in, fall steering, aim target updates, an animation state-machine condition, and AI steering override. Only two are swim.
