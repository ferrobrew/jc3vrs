# Swim yaw: the body flips 180° as soon as the swimmer moves

## Symptom

With smooth-swim-yaw engaged, turning while treading water worked exactly as intended, and the body went haywire the moment the left stick went forward — flipping roughly 180°, thrashing, and reading in VR as an unexitable spin.

## What the trace showed

A per-frame NDJSON trace, 893 frames over 29.7 s (the diagnostic scaffolding it came from was removed once the work landed; see the git history for `payload/src/debug/swim_trace.rs` and `tools/analyze_swim_trace.py`). Idle turning is clean: the body tracks the right-stick target with a one-frame lag at a steady 7.1°/frame (~200°/s), and stops dead on release.

```
 #    body      tgt     err   dbody  lookx     lstick
303  46.32    39.16   -7.17   -7.18   0.96  (+0.00,+0.00)
...  perfect tracking, body[i] == tgt[i-1]
317 -47.30   -47.30   -0.00   -1.11   0.00  (+0.00,+0.00)
```

Then the left stick comes in and it breaks in a single frame:

```
510  43.05    43.05   -0.00    0.00   0.00  (-0.05,+1.00)
511 -132.32   43.05  175.37 -175.37   0.00  (-0.06,+1.00)
```

Over the capture: 555 frames within 2° of the target, **147 within 10° of `target − 180°`**, the rest in transit between those two attractors. Not drift or oscillation — a binary 180° flip toggling with movement.

## Root cause

The `SetOrientation` chase rewrote the yaw of the matrix passed to `CPfxCharacterInstance::SetOrientation`, extracting the current yaw with a YXZ euler decomposition. **That matrix is not the character's facing frame.** From `UpdateSurfaceMovement` (`0x1407EA78F`):

```c
if ( v112 <= 0.0 || fsqrt(v135.x² + v136²) <= 0.1 )   // stick neutral / wanted speed ≤ 0.1
    v129 = CMatrix4f::operator*(a1 + 12420, v186, v181);   // idle  offset
else
    v129 = CMatrix4f::operator*(a1 + 12548, v168, v181);   // crawl offset
CMatrix4f::OrthoNormalize(&v164);
CPfxCharacterInstance::SetOrientation(*(a1 + 8272), &v164);
```

and `SSwimActionParams::SSwimActionParams` (`0x140779BF0`) gives those offsets their values:

| Offset | Value |
|---|---|
| `m_SurfaceIdleSwimmingCapsuleOffset` | identity |
| `m_SurfaceCrawlSwimmingCapsuleOffset` | **`RotationX(-1.5707951)`** — exactly −π/2 |

Stationary you get the bare facing matrix and the yaw extraction is valid, so the chase worked. Moving, the game swaps in the prone crawl capsule and the matrix is pitched 90° — **exact gimbal lock for a YXZ euler yaw**, where yaw and roll trade off freely and flip by 180° on noise. The chase's `delta` was garbage and the write landed at `target ± 180`. Wanted speed hovers near the 0.1 threshold, the capsule alternates frame to frame, and the body ping-pongs.

## The other half: the chase was solving the wrong problem

`DoRotate` (`0x1407754F0`) is called from two places per swim frame, and an earlier reading of the 2016 dump matched the wrong one:

- **`0x1407EA347`**, directly in `UpdateSurfaceMovement` — output feeds `CMatrix4f::CreateOrientation` at `0x1407EA49E`. **This is the body facing.**
- **inside `ProcessMotion`** (`0x140775760`, called at `0x1407E9AFC`) — the velocity-direction slerp.

So the `DoRotate` and `TransformInputDirToWorldDir` overrides already reached the facing: the transform's output becomes `DoRotate`'s target while moving, and `DoRotate` turns the body. The `SetOrientation` chase was redundant on top of a mechanism that already worked — and it was the only thing that broke.

Idle turning had masked this. With the stick neutral the core aims `DoRotate` at the character's *own* forward, so it does not rotate; the chase was doing all the idle turning, which is why removing it naively would have cost spin-in-place. Overriding `DoRotate`'s target unconditionally covers both cases.

## The fix

- Delete the `SetOrientation` chase, `CHASE_YAW`, and the `swim_turn_step_min_deg` / `swim_min_flat_forward_sq` config it needed.
- Keep the `DoRotate` target override (now the primary mechanism, covering idle *and* moving), the `TransformInputDirToWorldDir` substitution, and the `GetDeltaAngleFromOrientation` suppression — the last still matters, because a live turn act replaces the core's `DoRotate` rotation with the animated-turn slerp.
- Detour `ProcessMotion` and set a thread-local so the heading override skips its nested velocity `DoRotate`.
- Handle backward input, which the substitution had been quietly turning into forward motion. Reversing the direction was tried first and produced paddling in place, re-orientation, and sideways drift, so backward is now refused outright at its effector.

## Incidental fixes found along the way

- `xr::advance_body_yaw`'s `swimming` branch was dead: it wrote a clamped yaw to `s.yaw`, then an unconditional `s.yaw = Some(yaw)` two lines later overwrote it. Harmless (the unclamped value is what water wants), but it read as intent that was not happening.
- The trace recorded `velocity_yaw` and `camera_yaw` as `atan2(x, -z)` while `body_yaw` and `target_yaw` used `atan2(-x, -z)` — opposite sign. Negating the recorded value made `velocity_yaw` match `body_yaw` to three decimals. This made 426 of 526 shift frames classify as velocity/camera divergence when both were pure sign artifacts, burying the real signal.
- The raw `Character` offsets the trace was reading (motion state, animated-turn dirs, the pfx pointer) are now proper pyxis definitions: `character/swim_action_params.pyxis`, plus `m_PfxCharacter`, `m_CurrentMotionState`, and `m_SwimActionParams` on `Character`.

## Status

Fixed and validated in-headset. Three further problems surfaced behind this one and are covered in [mod/body/swim-yaw.md](../mod/body/swim-yaw.md): the shipped turn rates being far too slow for the stick, the underwater core needing a dive pitch, and the view amplifying head pitch because it rides the body frame.

