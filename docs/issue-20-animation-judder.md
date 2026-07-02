# Issue #20: Animations run at 30 Hz due to the decoupled sim tick

Research into the animation judder caused by JC3's decoupled simulation/render
design, and viable approaches to smooth character and object animations between
the 30 Hz simulation ticks.

## The problem

JC3's Apex engine decouples the simulation tick (30 Hz) from the render rate.
The mod forces `UpdateRender` every frame and drives a stereo double-`Draw`, but
the *simulation* — animation sampling, physics, AI — still ticks at the fixed
30 Hz rate. Character poses are sampled at 30 Hz and held for the render frame,
producing visible judder. This is most noticeable in first-person (Rico's own
body) and during vehicle entry transitions.

A global tick-rate change is not viable: the game's systems are designed for 30 Hz
and break at higher rates (missing delta-time multiplications, incorrect
behaviour). Whatever fix is applied must be targeted to the animation/pose
pipeline.

## The frame structure and interpolation system

### Decoupled sim/render architecture

`CGame::Update` (dump: `CGame.cpp`) implements the decoupled update loop. The
engine runs a fixed-step simulation and a variable-rate render. The key state
lives on the `CGame` struct (`CGame.h`):

| Field | Type | Role |
|---|---|---|
| `m_UpdateFrequency` | `int` | The fixed sim tick rate (30 Hz). |
| `m_DefaultUpdateFrequency` | `int` | Default tick rate (for resets). |
| `m_DecoupleEnabled` | `bool` | Whether sim and render are decoupled. |
| `m_InterpolationMethod` | `int` | Active interpolation method (see below). |
| `m_PrevInterpolationMethod` | `int` | Previous method (for re-init detection). |
| `m_InterpolationOverride` | `int` | Console override (-1 = no override). |
| `m_RunningFrameFraction` | `float` | Accumulated fractional tick (0.0–1.0). |

### Interpolation method values

The engine sets `m_InterpolationMethod` every frame in the decoupled path:

```c
if (CSequenceObject2::m_nCutscenesPlaying)
    this->m_InterpolationMethod = 2;  // cutscene: accumulator-based
else
    this->m_InterpolationMethod = 1;  // gameplay: single-step
if (this->m_InterpolationOverride >= 0)
    this->m_InterpolationMethod = m_InterpolationOverride;
```

The `switch (m_InterpolationMethod)` has three cases:

- **Method 1 (gameplay, default):** Runs `CGame::UpdateGame` at most once per
  render frame, accumulating the fractional timestep in
  `m_RunningFrameFraction`. The fraction is stored into the render context's
  `m_Dtf` field. This is the normal gameplay path.

- **Method 2 (cutscene):** Similar accumulator but runs multiple updates if
  the accumulator exceeds 1.0, subtracting 1.0 per update. Used during
  cutscenes for tighter sim/render coupling.

- **Method 3:** A time-based accumulator that computes the number of updates
  from wall-clock time (`QueryPerformanceCounter`). More aggressive — can run
  multiple sim updates per render frame if the render rate is low.

In all cases, the frame fraction (0.0–1.0) is stored at
`update_context.m_RenderContext.m_Dtf` (the `SGameObjectRenderContext.m_Dtf`
field, at byte offset 68 in the local `update_context_8` array — confirmed
from the struct layout: `SGameObjectRenderContext` has `m_Dt` at offset 0,
`m_Dtf` at offset 4).

The `m_Dtf` is the **interpolation alpha** between the previous sim tick
(T0) and the current sim tick (T1).

### How m_Dtf flows into the character render pipeline

The render context's `m_Dtf` reaches characters via
`CGameStateRun::UpdateRender`, which reads it at line 418:

```c
m_Dt = update_context->m_RenderContext.m_Dt;
m_Dtf = update_context->m_RenderContext.m_Dtf;
```

Then `CCharacter::UpdateRender` (dump: `CCharacter.cpp`, line ~42950) uses
`m_Dtf` in two critical places:

1. **Character world matrix interpolation:**

```c
CTransform::Lerp(
    (CTransform *)((char *)&v163.m256i_u64[1] + 4),
    m_Dtf,
    (const CTransform *)&this->m_GraphicsWorldT0.Translation.v[1]);
CTransform::ToMatrix4(..., &transform);
```

This lerps between `m_GraphicsWorldT0` (previous frame's character transform)
and `m_GraphicsWorldT1` (current frame's), producing a smooth interpolated
world position. This is the character *root* position interpolation, not the
per-bone pose.

2. **Skinning palette interpolation:**

```c
if ((*(_DWORD *)&this->m_LightWeight & 0x100) == 0)
    CCharacter::UpdateSkinning((CCharacter *)((char *)this - 8), m_Dtf);
```

`CCharacter::UpdateSkinning` (dump address `0x140_9DE_4B0`, release
`0x140_77E_150`) calls:

```c
NCharacterSystem::CAnimationControl::UpdateSkinning(
    this->m_AnimatedModel.m_AnimationController,
    dtf,
    &this->m_WorldMatrixT1);
```

### The skinning palette: T0/T1 pose interpolation

`CAnimationControl::UpdateSkinning` (release `0x140_43F_CA0`) is where the
per-bone pose interpolation happens. It calls:

```c
CPoseProducer::MakeSkinningPalette(this->m_Skinner.px, dtf, v13->Palette[v3 % 2]);
```

`CPoseProducer::MakeSkinningPalette` (release `0x140_C3A_FF0`) is the key
function. It:

1. Reads `m_PoseT0` (previous sim tick's `hkaPose`) and `m_PoseT1` (current
   sim tick's `hkaPose`) from the `CPoseProducer`.
2. Gets the model-space joint arrays from both poses.
3. Queues a skinning job that **blends between T0 and T1 by `dtf`** — the
   `dtf` parameter is the interpolation alpha (0.0 = previous pose, 1.0 =
   current pose).
4. The result is written into the skinning palette
   (`CSkinningPaletteData::Palette[frame_counter % 2]`), which is what the GPU
   vertex shader reads for skinning.

**This is the engine's built-in sub-frame pose interpolation.** The `hkaPose`
pair (T0/T1) is maintained by the sim phase: `UpdatePassFinalizePose_Parallel`
(release `0x140_7F9_B10`) finalizes T1, and the previous frame's T1 becomes
T0.

### The camera transform interpolation

The camera also uses T0/T1 interpolation. `Camera::UpdateRender` (documented
in `docs/rendering.md` §2.2) does:

```c
Lerp(&m_TransformF, &m_TransformT0, &m_TransformT1, dtf);
```

But the mod sets `m_TransformT0 = m_TransformT1` (via the
`always_use_t1` config or by writing T0 from T1), which makes the Lerp a
no-op — the camera transform is constant. This is intentional for the
first-person camera (the mod writes the head-bone position into both), but
it means the camera does not benefit from the engine's interpolation.

### SkipSubframeInterpolation

There is a `m_SkipSubframeInterpolation` flag on `CCharacter` that, when set,
causes `UpdatePassFinalizePose_Parallel` to bypass interpolation: it copies
T1 directly to T0 (`memcpy(&m_WorldMatrixT0, &m_WorldMatrixT1, ...)`) and
calls `SyncPoses` to sync the pose buffers. It is set when the character's
ragdoll orientation changes drastically between frames (the dot-product check
against `s_min_dot`), to avoid interpolation artifacts across large
rotational discontinuities.

### The mod's forced-UpdateRender path

The mod patches `Game::Update` to force `UpdateRender` every frame:

- `Game::Update + 0x787`: nop the `m_UpdateFlags & 4` check (always
  UpdateRender).
- `Game::Update + 0x7A2`: nop everything between UpdateRender and
  `++m_RenderCount` (the mod drives Draw itself).

This forces the *render* side to run every frame, but the *simulation*
(UpdateGame) still ticks at 30 Hz. The `m_Dtf` value is computed in the
decoupled loop and reflects the fractional position between sim ticks — so
the engine's interpolation *should* be working. The question is whether the
mod's path is preserving it.

## Viable approaches

### Approach A: Verify and preserve the engine's built-in interpolation

**Finding:** The engine already has a complete sub-frame pose interpolation
pipeline:

1. The decoupled loop computes `m_Dtf` (the frame fraction) and stores it in
   `SGameObjectRenderContext.m_Dtf`.
2. `CCharacter::UpdateRender` uses `m_Dtf` to:
   - Lerp the character world matrix between T0 and T1.
   - Call `UpdateSkinning(dtf)`, which calls `MakeSkinningPalette(dtf)` to
     blend the per-bone skinning palette between the T0 and T1 `hkaPose`.
3. The camera uses `Lerp(TransformF, T0, T1, dtf)` — but the mod nullifies
   this by setting T0 = T1.

**The critical question:** Is the mod's forced-UpdateRender path preserving
`m_Dtf`? The mod hooks `Game::UpdateRender` and drives the eye Draw loop
itself. The `m_Dtf` is set inside `CGame::Update`'s decoupled loop *before*
calling `UpdateRender`. If the mod's `Game::UpdateRender` detour receives
the `SUpdateContexts` with the correct `m_Dtf`, then the engine's
interpolation should already be active.

**Risk:** The mod's `Clock::Update` detour gates the clock to once per real
frame. This prevents the SPF exponential smoother from double-stepping, but
it does not affect `m_Dtf` — that is computed from `QueryPerformanceCounter`
deltas, not the clock. However, if the mod's frame structure causes
`m_RunningFrameFraction` to be computed incorrectly (e.g., the mod skips
the decoupled loop's accumulator and calls UpdateRender directly with a
stale or zeroed `m_Dtf`), the interpolation alpha would be wrong.

**Investigation steps:**
1. Log `m_Dtf` in the `Game::UpdateRender` detour to confirm it is non-zero
   and varying between 0.0 and 1.0 across render frames. If it is always 0.0
   or always 1.0, the mod's path is bypassing the accumulator.
2. Log `m_RunningFrameFraction` on the `Game` struct to confirm the
   accumulator is advancing.
3. If `m_Dtf` is correct, the issue may be that the *camera* path is
   nullifying the interpolation (T0 = T1), while the *body* is actually
   interpolating correctly — and the perceived judder is the camera
   snapping while the body smooths.

**If `m_Dtf` is wrong:** The fix is to compute the correct frame fraction
in the mod's `Game::UpdateRender` detour before calling the original, and
write it into `update_context->m_RenderContext.m_Dtf`. The fraction is
`m_RunningFrameFraction` (clamped 0.0–1.0) from the `Game` struct, or can
be computed from the clock's SPF and the real frame time.

### Approach B: Re-sample the animation between sim ticks

If the engine's interpolation is insufficient or disabled, the mod could
re-sample the animation pose between sim ticks in the existing
`character_update_prop_effects` hook.

**Finding:** The `character_update_prop_effects` hook (release
`0x140_7C2_380`) runs in the SIM phase, after pose finalization. It has
access to the `AnimationController` and can call `GetJoint`/`SetJoint` to
read and write the model-space pose. The hook already does `SetJoint` (the
head-hide hack), proving the override reaches the render.

**However:** The skinning palette (what the GPU actually uses) is built by
`MakeSkinningPalette(dtf)` in the RENDER phase (`CCharacter::UpdateRender`),
not in the SIM phase. The `UpdatePropEffects` hook writes to the T1 pose,
and the T0 pose is the previous frame's T1. The interpolation happens in
`MakeSkinningPalette` using `dtf`.

So the correct place to inject a custom interpolation is not
`UpdatePropEffects` (which writes the T1 pose), but rather:

1. **Before `MakeSkinningPalette`** — write a custom T0 that is a blended
   pose, or override `dtf` to control the blend ratio. This is essentially
   Approach A.

2. **After `MakeSkinningPalette`** — directly patch the skinning palette
   (`CSkinningPaletteData::Palette[frame % 2]`) with a blended result. This
   is more invasive and requires understanding the palette's memory layout
   (an array of `CMatrix3x4f` per bone).

3. **Override `UpdateSkinning`** — hook `CCharacter::UpdateSkinning` (release
   `0x140_77E_150`) or `CAnimationControl::UpdateSkinning` (release
   `0x140_43F_CA0`) and pass a custom `dtf`. This is the simplest
   intervention: if the engine's `dtf` is wrong (e.g., 0.0 or 1.0), the hook
   can compute the correct sub-tick alpha and pass it instead.

**Recommended sub-approach:** Hook `CAnimationControl::UpdateSkinning`
(release `0x140_43F_CA0`) for the local player character only, and compute
the correct `dtf` from the real frame time and the known sim tick rate
(1/30). This gives per-bone pose interpolation without touching the pose
buffers or the skinning palette directly.

```c
// dtf = fractional position between sim ticks
// sim_tick = 1.0 / m_UpdateFrequency  (1/30 = 0.0333...)
// dtf = (time_since_last_sim_tick) / sim_tick, clamped [0, 1]
```

### Approach C: Pose sampling vs. consumption (camera path)

**Finding:** The pose is finalized in SIM (`UpdatePassFinalizePose_Parallel`)
and consumed in RENDER (`CCharacter::UpdateRender` → `UpdateSkinning(dtf)`).
The camera reads `GetSafeBoneMatrix(HEAD)` in `Camera::UpdateRender`, which
reads from the model-space pose buffer.

The `m_WorldMatrixT0` / `m_WorldMatrixT1` fields on `Character` are the
character *root* world transforms for the previous and current sim ticks.
The camera hook already reads T1 (or T0 via `always_use_t1`).

**The issue may be that the camera reads the pose at the wrong time:**
- If the camera reads the T1 pose (current sim tick) without
  interpolation, and the body is rendered with T0→T1 interpolation (via
  `MakeSkinningPalette(dtf)`), the camera and body are out of sync.
- The mod's camera hook writes to `m_TransformT0` and `m_TransformT1` on
  the active camera, and the engine lerps between them. If the mod sets
  T0 = T1, the camera position is constant (no interpolation), while the
  body interpolates — producing a mismatch where the body judders relative
  to the camera.

**Fix:** If the body is interpolating correctly (Approach A confirms `dtf`
is valid), the camera should also interpolate. Instead of setting T0 = T1,
the mod should set T0 to the previous frame's head position and T1 to the
current frame's, letting the engine's `Lerp(TransformF, T0, T1, dtf)` smooth
the camera position. The risk is that HMD-driven head movement should be
1:1 with no interpolation lag (per `docs/head-and-body.md` — "no smoothing
on the HMD→camera path"), so this approach trades body smoothness for head
lag. The correct split: interpolate the *body root* position (which comes
from the sim and judders), but keep the *HMD-driven head offset* 1:1.

## Summary of findings

| Question | Answer |
|---|---|
| Does the engine have pose interpolation? | Yes — `MakeSkinningPalette(dtf)` blends between T0 and T1 `hkaPose` objects by the frame fraction `m_Dtf`. |
| Where is `m_Dtf` computed? | In `CGame::Update`'s decoupled loop, stored in `SGameObjectRenderContext.m_Dtf`. |
| Is the mod's path preserving it? | Unknown — needs runtime logging. The mod forces UpdateRender but may be passing a stale/zeroed `m_Dtf`. |
| Where does the camera nullify interpolation? | The mod sets `m_TransformT0 = m_TransformT1` (via `always_use_t1` or by writing both from the same head position), making the camera Lerp a no-op. |
| Is `m_SkipSubframeInterpolation` relevant? | Only for ragdoll rotational discontinuities — not the general judder. |
| Can the mod inject a custom `dtf`? | Yes — hook `CAnimationControl::UpdateSkinning` (release `0x140_43F_CA0`) and pass a computed sub-tick alpha. |

## Recommended next steps

1. **Log `m_Dtf` and `m_RunningFrameFraction`** in the `Game::UpdateRender`
   detour to determine whether the engine's interpolation is active. This is
   the single most important diagnostic — it distinguishes "interpolation is
   working but we can't see it" from "interpolation is disabled by the mod's
   path."

2. **If `m_Dtf` is wrong:** compute the correct sub-tick alpha in the mod
   and write it into the render context before calling the original
   `UpdateRender`. The alpha is `(real_frame_time % sim_tick_period) /
   sim_tick_period`.

3. **If `m_Dtf` is correct but the camera nullifies it:** consider letting
   the camera position interpolate (set T0 to the previous frame's position)
   while keeping the HMD rotation 1:1. The head-bone override (issue #5)
   will change this landscape — once the head is HMD-driven, the body root
   interpolation is the only remaining judder source.

4. **If the engine's interpolation is fundamentally insufficient** (e.g.,
   it only interpolates the root, not the bones): hook
   `CAnimationControl::UpdateSkinning` and verify that
   `MakeSkinningPalette` is actually blending the bones, not just the root.

## Key release addresses

| Symbol | Release address |
|---|---|
| `CCharacter::UpdatePropEffects` | `0x140_7C2_380` |
| `CCharacter::UpdatePassFinalizePose_Parallel` | `0x140_7F9_B10` |
| `CCharacter::UpdateSkinning` | `0x140_77E_150` |
| `CAnimationControl::UpdateSkinning` | `0x140_43F_CA0` |
| `CPoseProducer::MakeSkinningPalette` | `0x140_C3A_FF0` |
| `CCharacter::GetRenderTransform` | `0x140_75D_E50` |
| `CCharacter::IsInDrivingVehicleState` | `0x140_77E_AF0` |
| `CShadowManager::CommitRenderPassSettings` | `0x140_177_9C0` |
| `CRenderPass::SetRenderContextCamera` | `0x140_187_430` |
