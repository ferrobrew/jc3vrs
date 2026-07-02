# JC3 (Apex engine) per-frame rendering pipeline — authoritative reference

Reverse-engineered from the 2025 Denuvo-less Steam build of Just Cause 3 (Apex engine). Where a claim could not be confirmed it is flagged **[UNVERIFIED]**. The concrete addresses, struct offsets, singletons and globals all live in the pyxis-defs (the `graphics_engine/`, `camera/`, `ui/` modules); this doc is the wiring — what each thing is, what reads what, and the frame order. Reference everything here by name and follow the name into the defs for the exact address/offset.

Data layout (struct/field offsets, singletons, globals) is byte-stable across the Denuvo / Denuvoless / debug builds — only `.text` (function addresses) moved. So the offsets are valid on every build; only the function addresses are build-specific.

For the shader side — extracting the DXBC from the bundles, disassembling it, finding a specific effect, and how the mod patches shaders in memory — see [`shaders.md`](shaders.md).

Key singletons (this build):
- `GraphicsEngine` instance: the `RenderEngine` singleton aliases the same pointer (both `qword_142ED0E18` and `qword_142E2B6F0` appear for it).
- `CameraManager` (engine-side) instance.
- `GameCameraManager` instance: a distinct singleton (`Base::CSingle<GameCameraManager>::Instance`), separate from the engine-side `CameraManager` it drives.
- `Clock` instance.

---

## 1. Threading & frame structure

### 1.1 Two threads

- Main / sim thread runs `Game::Update` -> `Game::UpdateRender` -> `Game::Draw` -> `GraphicsEngine::Draw`.
- Render / draw worker thread runs `GraphicsEngine::HandleDrawThreadTask`, which builds and submits the actual GPU command stream for the frame.

The split is decided in `GraphicsEngine::DispatchDraw`:

```c
if ( CpuPrimaryCount() <= 1 )
    return HandleDrawThreadTask(a1, &unk_142ED0F68);          // inline, same thread
// else: post a fragment job "CallProxy" to the worker pool
v2 = HashString("CallProxy");
CpuFragmentCall(v2, v4 /* {fn=sub_1400F2C70, ctx, signal} */, 24, &v5, 0);
```

So with >1 CPU primary, `DispatchDraw` enqueues a `CpuFragmentCall` that eventually reaches `HandleDrawThreadTask` on a worker; with a single primary it runs inline. The work performed is identical either way — only the thread differs.

### 1.2 What `WaitForCPUDrawToFinish` drains

```c
if ( *(a1+32) ) CpuFragmentWaitUntilSignalIsNonZero();   // wait on the shared_ptr<SCallProxyKeepAlive> end-draw signal
if ( !*(BYTE*)(a1+20) )                                   // a1+20 = "draw done" event-set flag
    WaitForSingleObject(*(HANDLE*)(a1+24), INFINITE);     // wait on the draw-done Win32 event
```

`GraphicsEngine::WaitForCPUDrawToFinish` blocks until the previous dispatched `HandleDrawThreadTask` has finished (the end-of-draw `SetEvent` at the tail of `HandleDrawThreadTask` sets `m_CPUFinishedDrawingEvent`/its flag). It is the fence between the render worker and the next frame's sim work. `Game::Draw` calls it at entry and `GraphicsEngine::Draw` re-checks the same two conditions in its prologue.

### 1.3 Prologue of `GraphicsEngine::Draw` — per-real-frame-once state

Executed once per real frame, in order (offsets are relative to the `GraphicsEngine::Draw` entry):

| Site | Operation | Notes |
|---|---|---|
| `Draw+0x31` | wait on the fragment signal + draw-done event | drain previous frame |
| `Draw+0x54` | `*(float*)(engine+2736) = dt` | store the frame dt for the worker |
| `Draw+0x66` | `render_frame_counters.m_FrameIndex = render_frame_counters.m_Counter++` | **RenderFrameCounter / parity source**: `m_FrameIndex` = previous value of `m_Counter`, then `m_Counter` increments |
| `Draw+0x8C` | `unk_142E5B664 = m_Counter % 3` | a `%3` ring index |
| `Draw+0x99` | `unk_142E5B670 = m_RingIndex` | save previous |
| `Draw+0xB4` | `render_frame_counters.m_RingIndex = m_FrameIndex % 3` | the per-frame `%3` ring used by RBI/CB indices |
| `Draw+0xC2` | `++*(engine+5824); if (>= *(engine+5828)) =0` | a ring index at engine `+5824`. **NOT a back-buffer ring** — it feeds the GPU-profiler frame-query ring (consumed by `CGPUProfiler::EndFrame`). The real presentable surfaces are owned by the DXGI swapchain; see §7.1. |
| `Draw+0xE4` | `TextureCachePreUpdate` | texture streaming bookkeeping |
| `Draw+0x110` | `CGameStateRun::UpdatePostEffectEdit` | post-effect-edit (dev) sync |
| `Draw+0x11E` | `GraphicsEngine::TextureCachePlatformUpdate(engine, masterCtx)` | **the camera copy + render-camera setup happens here — see §2** |
| `Draw+0x21D` | `Clock::Update` | **the per-frame clock tick** (the slow-mo hazard, see PLAN §5.4) |
| `Draw+0x229` | `CConstantBufferPool::HandBackBuffers` | CB pool ring rotation |
| `Draw+0x23F` | `CRenderEngine::CalculateConstantBufferIndices` | per-RBI CB draw indices |
| `Draw+0x247` | `DispatchDraw` | hand the frame to the worker |

The present (Flip) is in this prologue. At `Draw+0x1F6`..`Draw+0x207`: `if (!*(BYTE*)(engine+4799)) GraphicsEngine::Flip(engine); *(BYTE*)(engine+4799) = 0;` — `GraphicsEngine::Flip` does `AcquireThreadOwnership; graphics_flip(device); ReleaseThreadOwnership` (verified: it calls `graphics_flip` in its body). So the prologue presents the previous frame's back buffer, guarded one-shot by the `engine+4799` flag (set elsewhere to skip the very first frame). This is the pipelined-present model PLAN.md describes: Flip-previous, then `DispatchDraw` the current frame. The VR `BLOCK_FLIP` must suppress `graphics_flip` — hooking it directly is correct since this prologue reaches it through `GraphicsEngine::Flip`.

### 1.4 Body of `HandleDrawThreadTask` — per-dispatch

High-level order (offsets are relative to the `HandleDrawThreadTask` entry):

1. `+0x77` `QueryPerformanceCounter` -> `unk_142ED0F58` (draw start time).
2. `+0x95` get master context (`GetMasterContext`) -> `engine+2616`.
3. `+0xB8` `CShadowManager::CommitRenderPassSettings` (if `unk_142ED75BB`, i.e. shadows enabled).
4. `+0xF5` `TextureCacheGpuUpdate`.
5. `+0x12A` `CRenderPass::SetupRenderContext(engine+1824, 0, ctx)` — `engine+1824` is the main `RenderContext`.
6. `+0x153` `CRenderPass::SetRenderContextCamera(engine+1824, CameraManager.m_RenderCamera)` — feeds the render camera (see §2) into the render context.
7. `+0x166` `CRenderEngine::SetGlobalShaderConstants(engine+1824)` — uploads per-view CB.
8. `+0x179` `SetAllGlobalShaderProgramConstants`.
9. If `engine+4798` (m_DoMainDraw) set:
   - UI static-background / sync (`GetIUIManager` vtable +40, +8).
   - `+0x234` `PreDraw`.
   - `+0x259` `CRenderEngine::DrawGBuffer` (passes `0x2F..0x55`).
   - `+0x267`..`+0x341` **EffectInfo slot machine** (see §5.1).
   - `+0x35D` `CRenderEngine::Draw` (passes `0x56..0x95`: lighting/SSR/reflection/main).
   - `+0x3BA` `CPostEffectsManager::ApplyWorldFilters(dt, ...)`.
   - `+0x3D2` `CRenderEngine::DrawPosteffects` (pass `0x96`).
   - else (no main draw): just UI submit.
10. `+0x475` `CPostEffectsManager::ApplyGlobalFilters(dt, ctx)`.
11. `+0x486` `CRenderEngine::PostDraw`.
12. `+0x4A9`/`+0x66D` GPU-profiler `EndFrame` (one of two countdown branches), screenshot countdown `engine+136`.
13. `+0x4FE` UI render block (clear + IUIManager vtable +48/+16/+24).
14. `+0x57C` `QuickDrawController::Draw` (debug overlays).
15. `+0x5A6` `CopySurfaceToTexture(engine+2616)` — final composite copy to the presentable surface (this is the engine-internal "blit to back buffer" the VR redirect intercepts; symbolized `ResolveSurface`, see §12).
16. `+0x5B2`..`+0x65C` end-draw one-shot signal callbacks (`sig::Signal`) — self-removing.
17. `+0x6B3` `CRenderEngine::EraseAllDeletedRenderBlocks`.
18. `+0x724`/`+0x789` compute draw-time floats (`engine+4784`, `engine+4788`).
19. `+0x791`..`+0x799` `*(engine+20)=1; SetEvent(*(engine+24))` — signal draw-done.

### 1.5 Per-frame vs per-dispatch

- **Per real frame (once), in `GraphicsEngine::Draw` prologue:** frame counter/parity (`render_frame_counters`), `%3` ring, the GPU-profiler frame-query ring (`engine+5824`), texture-cache pre/platform update, the active->render camera copy, `Clock::Update`, CB pool `HandBackBuffers`, CB indices.
- **Per dispatch (would run twice if you Draw twice), in `HandleDrawThreadTask`:** the entire render pass chain, the EffectInfo slot machine (§5.1), `ApplyWorldFilters`/`ApplyGlobalFilters` accumulators, screenshot countdown, profiler frame begin/end, the final copy + end-draw callbacks.

---

## 2. Camera data flow

### 2.1 Cast of cameras

- `GameCameraManager` holds `m_ControlContext` with four sub-contexts (Previous/Next x Camera/Render); these carry `m_CameraTransform` (a world transform, not a view matrix), FOV, blur factors.
- `CameraManager` (engine) holds:
  - `m_ActiveCamera`: the active scene camera (a `Camera`, 0x5B0 bytes).
  - `m_RenderCamera`: the camera the render thread reads.
- A `Camera`'s matrix/flag layout is in the `Camera` def (`m_PreviousTransformF`, `m_TransformF`, `m_TransformT0`, `m_TransformT1`, `m_ProjectionF`, `m_ViewProjectionF`, `m_Projection`, `m_View`, `m_ViewProjection`, ...). The `m_StateBitfield` (`CameraState`) carries `m_ComputeView` (0x08, compute-view-from-transform), `m_DirtyProjection` (0x10), and `m_IsRenderCamera` (0x20, the SetupRenderCamera one-shot guard).

### 2.2 Sim path: where `m_ActiveCamera.m_View` is produced (once per frame)

In `GameCameraManager::UpdateRender`, runs in the sim path (under `CGameStateRun::UpdateRender`, before `Game::Draw`):

```
CCameraTree::UpdateRenderContexts(...)         // populate m_ControlContext contexts
CSpecialCameras::UpdateRender(...)
GameCameraManager::PushRenderContext(this)     // -> InitTransform: m_ActiveCamera.T0 = T1 = ctx transform
CameraManager::UpdateRender(dt, dtf)
```

`PushRenderContext` eventually calls `CameraManager::InitTransform`:

```c
v2 = m_ActiveCamera;
memcpy(v2 + 148 /*+0x94 T0*/, mat, 0x40);
memcpy(v2 + 212 /*+0xD4 T1*/, mat, 0x40);     // T0 = T1 = context transform
```

So the gameplay camera's *world transform* lands in `m_ActiveCamera.m_TransformT0 == m_TransformT1`.

Then `CameraManager::UpdateRender` iterates every camera in its list and calls `Camera::UpdateRender`. That function is where `m_View` is computed:

```c
m_PreviousView = m_View;  m_PreviousProj = m_Projection;  m_PreviousViewProjection = m_ViewProjection;
m_PreviousTransformF = m_TransformF;
Lerp(&m_TransformF, &m_TransformT0, &m_TransformT1, dtf);     // T0==T1 => constant
if ( flag & 0x08 )                                            // SetComputeView(true)
    m_View = m_TransformF;
    CMatrix4f::Inverse(&m_View);                              // m_View = inverse(world transform)
...
Multiply4x4(&m_View, &m_Projection, &m_ViewProjection);
m_ProjectionF = m_Projection;  m_ViewProjectionF = m_ViewProjection;  ...
```

`Camera::SetComputeView(true)` sets flag bit `0x08`. The active gameplay camera has this set, so `m_View` is re-derived as `Inverse(m_TransformF)` every sim frame, from the T0/T1-Lerped transform. Because `InitTransform` set T0 == T1, the Lerp is a no-op constant and engine pose interpolation is effectively disabled for the active camera (independent of `m_InterpolationOverride`).

`m_View` is computed exactly once per real frame, in the sim path, here. It is NOT recomputed at Draw time.

### 2.3 Draw path: active -> render camera copy

At Draw time the prologue calls `GraphicsEngine::TextureCachePlatformUpdate`. Near its top:

```c
memcpy((char*)this + 368, CameraManager.m_ActiveCamera, 0x5B0);   // render-copy = *m_ActiveCamera (whole Camera)
Camera::SetupRenderCamera(this + 368, 1);                         // derive VP, reverse-Z, jitter
CameraManager.m_RenderCamera = this + 368;
```

So the render camera lives inside the `GraphicsEngine` object at `engine+368` and is a byte-for-byte copy of `m_ActiveCamera` (including its `m_View`), made once per frame.

`Camera::SetupRenderCamera` is guarded by flag bit `0x20` (`m_IsRenderCamera`). The body (verified):

```c
result = m_StateBitfield;
if ( (result & 0x20) == 0 ) {                 // check-AND-SET: skip entirely if already 0x20
    m_StateBitfield |= 0x20;
    // reverse-Z fixup on m_Projection (z' = w - z): e[2]=e[3]-e[2]; e[6]=e[7]-e[6]; e[10]=e[11]-e[10]; e[14]=e[15]-e[14]
    // same reverse-Z on the secondary/previous projection:
    if ( a2 ) {                               // a2 = "apply jitter" (always 1 from the prologue call)
        ApplyJitterTransform(engine, m_Projection, ...);
        ApplyJitterTransform(engine, m_PreviousProj, ...);
    }
    Multiply4x4(m_PreviousView, m_PreviousProj, m_PreviousViewProjection);
    Multiply4x4(m_View,         m_Projection,   m_ViewProjection);
    m_ProjectionF     = m_Projection;
    m_ViewProjectionF = m_ViewProjection;
    // + the secondary projection / VP float-shadow copies
}
return result;                                 // if 0x20 already set: NO-OP
```

The guard is check-and-set: the body runs only when bit `0x20` is clear, and sets `0x20` as its first action. The active/source camera (`m_ActiveCamera`) never has `0x20` set — `Camera::UpdateRender` (the sim-path per-frame update) asserts `m_IsRenderCamera == false`, so the active cam carries `0x20` clear. The Draw-prologue memcpy copies it verbatim, so the render copy (`engine+368`) always lands with `0x20` clear; `SetupRenderCamera` then runs its full body (reverse-Z + jitter + VP rebuild) and sets `0x20` on the copy. `m_ViewProjection` / `m_ViewProjectionF` are therefore rebuilt from `m_View x m_Projection` on every Draw. A second call on the same object (a double dispatch reusing the copy) is a no-op, which constrains hook ordering (§2.5).

`SetupRenderCamera` reads `m_View` as an input and never writes it. It does not derive `m_View` from any transform. So whatever `m_View` was in the copied active camera is what the render camera uses. Corollary for VR: a hook that writes the render-cam `m_View` *after* `SetupRenderCamera` has already run (`0x20` set) will leave a stale `m_ViewProjection`/`m_ViewProjectionF`, because the re-call is a no-op and nothing else rebuilds VP on the render cam. You must then rebuild VP yourself (§2.6).

`ApplyJitterTransform` -> `CAntiAliasingEffect::ApplySubsampleJitter` only fires when AA mode `== 3`; it post-multiplies a sub-pixel clip-space translation onto the projection (`proj = proj · jitterMat`).

### 2.4 What the scene render actually reads (camera-relative)

In `HandleDrawThreadTask`:

```c
SetRenderContextCamera(engine+1824, CameraManager.m_RenderCamera)   // = engine+368
SetGlobalShaderConstants(engine+1824)
```

The main scene is rendered **camera-relative** (for large-world float precision). The opaque-geometry transform is

```
clip = (objectWorld - CameraPosition) x OffsetViewProjection
```

where `OffsetViewProjection` is the view-projection with the view's **translation row zeroed**, and `CameraPosition` is the camera's **world position** supplied separately. The camera translation lives in the per-object subtraction, not in the matrix.

`CRenderPass::SetRenderContextCamera` (non-shadow path) reads the live render camera (`engine+368`) and fills the render context (`engine+1824`):

- ctx ViewMatrix <- camera `m_View` (`SetRenderContextCamera+0x4AD`)
- ctx ProjectionMatrix <- camera `m_ProjectionF` (`SetRenderContextCamera+0x50C`)
- ctx **CameraPosition** <- the translation row of `m_TransformF` (the camera world position) (`SetRenderContextCamera+0x5C2`)
- ctx <- the full `m_TransformF` world-transform rows
- `CalculateOffsetViewProjectionMatrix` is called twice, current + previous (`SetRenderContextCamera+0x56A`/`+0x584`): it copies `m_View`, **zeros its translation row** (`row3 = {0,0,0,1}`), multiplies by the projection, and writes the translation-free **OffsetViewProjection** into the context. This is the VP opaque geometry actually uses.

`CRenderEngine::SetGlobalShaderConstants` uploads the **global per-view CB**: it calls `CameraManager::GetRenderCamera` then `memcpy(this+6428, RenderCamera.m_ViewProjectionF, 0x40)` (`SetGlobalShaderConstants+0x757`) — the *full*, translation-bearing `m_ViewProjectionF`; the camera-position constants come from `m_TransformF`'s translation. This per-view block drives **screen-space / non-geometry** work (post-effects, billboards, the camera-position constant); it is **not** what positions opaque geometry vertices.

**Consequence for VR (the key correction):** on the geometry path, `m_View`'s translation is deleted by the OffsetVP zeroing. A per-eye lateral offset must move the **camera world position** — `m_TransformF`'s translation, hence `CameraPosition` — not `m_View`'s translation. (Confirmed at runtime: writing only `m_View`'s translation gave two divergent `m_ViewProjectionF`s on the render camera but byte-identical geometry between eyes; only the muzzle flash, which rides the per-view CB, shifted.) See §2.5.

### 2.5 Per-eye view injection

Because the geometry is camera-relative (§2.4), the per-eye offset must move the camera **world position** (`m_TransformF`'s translation), not `m_View`'s translation (which the OffsetVP zeros). It targets the **render camera** (`engine+368`), not `m_ActiveCamera` — the sim recomputes the active camera's `m_View = Inverse(m_TransformF)` (flag 0x08) and the prologue memcpy copies it verbatim, so any per-eye divergence has to be applied to the render-camera copy between dispatches.

**Recipe.** Hook `Camera::SetupRenderCamera` and act only on the main render camera (`this == GraphicsEngine + 0x170`), or equivalently hook right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. Per eye:

1. Offset `m_TransformF`'s translation by +/- IPD/2 along the camera's right axis (the first basis row of `m_TransformF`). This moves `CameraPosition`, so the camera-relative geometry diverges.
2. Re-derive `m_View = Inverse(m_TransformF)` so the view stays consistent with the moved camera (the OffsetVP's rotation comes from `m_View`; the per-view CB's `m_ViewProjectionF` uses the full `m_View`).
3. Rebuild `m_ViewProjection` / `m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.

Pitfalls:

- Offsetting only `m_View`'s translation does nothing to geometry — the OffsetVP zeros it (§2.4). It shifts only the per-view CB (post / camera-position constant), which is the muzzle-flash-moves-but-world-doesn't symptom.
- Offsetting only `m_TransformF` without re-deriving `m_View` desyncs `CameraPosition` from the OffsetVP rotation and the per-view CB.

### 2.6 The VP-rebuild recipe

The engine's `Matrix4` (`CMatrix4f`) is D3D-style — row-major, row-vector (`clip = p · M`) — and `Matrix4::Multiply4x4(a, b, dest)` writes `dest = a · b`; the full convention (basis-in-rows, the glam bridge) is documented on the `Matrix4` def. `SetupRenderCamera` calls `Multiply4x4(m_View, m_Projection, m_ViewProjection)` ⇒ `m_ViewProjection = m_View · m_Projection`, `clip = p · View · Projection`, and `Camera::UpdateRender` uses the identical order. So to rebuild a render camera after writing a custom `m_View` and/or `m_Projection`:

```c
Camera *cam = engine + 368;                                  // == CameraManager.m_RenderCamera
// (write cam->m_View and/or cam->m_Projection first)
Multiply4x4(&cam->m_View, &cam->m_Projection, &cam->m_ViewProjection);
memcpy(&cam->m_ProjectionF,     &cam->m_Projection,     0x40);
memcpy(&cam->m_ViewProjectionF, &cam->m_ViewProjection, 0x40);
```

The `*F` float-shadow copies (`m_ProjectionF`, `m_ViewProjectionF`) are what the render context and global CB actually read (§2.4) — you MUST update them too, not just `m_ViewProjection`.

### 2.7 Reverse-Z / depth convention + the per-eye projection wedge bug

The base projection built by `Camera::RecalcProjection` (via `PerspectiveFov` / `PerspectiveOffCenter` / `Ortho`) is a standard (non-reversed) projection. The reverse-Z remap `z' = w - z` is applied exactly once per render camera, gated by bit `0x20`:

- `RecalcProjection` applies it only when `m_IsRenderCamera` (0x20) is set — i.e. only when called on a render cam.
- `SetupRenderCamera` applies it **unconditionally to whatever is in `m_Projection`** when its `0x20`-guard body runs (§2.3). There is no "already reversed?" check.

Result: reverse-Z is applied once, by whichever of {`RecalcProjection`-on-render-cam, `SetupRenderCamera`} first sees `0x20` clear. Depth convention: reverse-Z, NDC z in [0,1] with near=1, far=0 (MainDepth is `D32FS8`), enabling far-plane precision / infinite-far style projections.

**The wedge bug (VR):** if you write a per-eye off-axis projection into the render cam's `m_Projection` **before** `SetupRenderCamera` runs, `SetupRenderCamera` will reverse-Z it. So:

- **Preferred:** supply a standard (non-reversed) off-axis projection into `m_Projection` *before* `SetupRenderCamera`, and let the engine apply reverse-Z + jitter once. Matches engine convention; you also get TAA jitter for free.
- **Alternative:** supply an already-reverse-Z'd projection *after* `SetupRenderCamera` (when `0x20` is set, so it won't re-reverse), then rebuild VP/`*F` yourself (§2.6). With this path you must apply jitter yourself if AA mode 3 is active, and maintain `m_PreviousProj`/`m_PreviousViewProjection` for velocity. Do not feed an already-reversed projection into the pre-`SetupRenderCamera` window — that double-applies the remap (the wedge artifact).

`CAntiAliasingEffect::ApplySubsampleJitter` only fires when AA mode `== 3`; it post-multiplies a sub-pixel clip-space translation (`m[12]=jx`, `m[13]=jy`, `j = ±offset/width,height`) onto the projection: `proj = proj · jitterMat`.

---

## 3. Render passes / stages (ordered)

The pass system is a flat enum `ERenderPass` of ~180 `RP_*` values (ordered, contiguous; names recovered from the pass-name switch). Passes are drawn by index range via `CRenderEngine::DrawRenderPassRange(ctx, renderSetup, first, last)`, which walks `CRenderEngine::m_RenderPasses[]` (a fixed array of 157 `CRenderPass*` at `renderEngine + 32*pass + 128`, created at init) and vtable-dispatches each pass's render blocks. The per-frame render-block instances live in each `CRenderPass`'s double-buffered `m_Lists[2]` (two `CRBILists`); see §11.

Ordered stages per dispatch:

1. **Shadow atlas** — committed in `CShadowManager::CommitRenderPassSettings` and rendered as the `RP_SHADOW_0..7` / `RP_STATIC_SHADOW_0..7` / `RP_SHADOW_REFLECTIVE_*` passes. Shadow data is **parity-buffered** by `render_frame_counters.m_FrameIndex & 1`: `CRenderPass::SetRenderContextCamera` and the shadow-matrix reads index shadow storage by `(m_FrameIndex & 1) << 8` (a 256-byte stride per parity) at `SetRenderContextCamera+0xEA`, `+0x1C3`, `+0x726`. So shadows ping-pong between two parity buffers each real frame. Running Draw twice in one real frame uses the same parity twice (parity only advances in the `GraphicsEngine::Draw` prologue), which is benign for the second eye but means the second eye sees the same parity slot.

2. **GBuffer fill** — `CRenderEngine::DrawGBuffer` -> `DrawRenderPassRange(setup, 0x2F, 0x55)`. The depth/velocity prefix is `RP_Z_OCCLUDERS`, `RP_Z_COARSE_PASS`, `RP_Z_PASS`, `RP_Z_AND_VELOCITY_PASS` (this is where the velocity buffer is written using the previous-frame VP), then static/dynamic models, then `RP_DECALS` / `RP_SCREEN_SPACE_DECALS` / `RP_SCREEN_SPACE_ROAD_DECALS`, terminating at `RP_LAST_GBUFFER` (0x55). Before drawing it binds two FS textures (slots 0x28, 0x29).

3. **Lighting / reflections / main** — `CRenderEngine::Draw` -> `DrawRenderPassRange(setup, 0x56, 0x96)`. In order: `RP_REFLECTIVE_WATER_PLANES`, `RP_AO_VOLUMES`, `RP_SSAO`, `RP_SCREEN_SPACE_REFLECTIONS`, `RP_GLOBAL_ILLUMINATION`, `RP_SCREEN_SPACE_SUBSURFACE_SKIN`, `RP_DEFERRED_LIGHTS` (these resolve lighting into MainColor), then opaque/`RP_LAST_OPAQUE`, environment (`RP_STARS`/`RP_SUN`/`RP_MOON`/`RP_SKYBOX`), water, transparency, ending just before `RP_POSTEFFECTS`. Reflection-proxy passes (`RP_REFLECTION_*`) and `RP_ENVREFLECTION` are also in this block.

4. **Post-effects (world)** — `CPostEffectsManager::ApplyWorldFilters` enqueues the world post-effect block, then `CRenderEngine::DrawPosteffects` -> `DrawRenderPassRange(setup, 0x96, 0x97)` runs pass `0x96` (`RP_POSTEFFECTS`). The actual HDR chain executes inside `CRenderBlockPostEffects::Draw`, in order:
   1. `CToneMappingEffect::GenerateHistogramForFinalScene` (exposure histogram)
   2. `CSunHaloEffect::PreApply`
   3. blur: bokeh path (`CDownScale2x2PackFocus::Apply` -> `CBlurEffectBokeh::Apply`) if `CPostEffectsManager::IsBokehActive`, else `CBlurEffect::Apply`
   4. `CGlareEffect::Apply`
   5. `CDepthOfFieldEffect::Apply`
   6. `CMotionBlurEffect::Apply` (gated by motion-blur active / AA-mode==3 / heat-haze)
   7. `CToneMappingEffect::DrawHistogramWindow` (**the HDR->LDR tonemap composite**)
   8. `CPlayerDamageEffect::Apply` (if damage flag set)
   9. `CAntiAliasingEffect::Apply`
   10. `CSunHaloEffect::Apply` + additive sun blend (`SetBlendFunc(5,5,6)`)
   11. `CFadeEffect::Apply` (final fade)

5. **Global filters** — `CPostEffectsManager::ApplyGlobalFilters` enqueues `RP_POSTEFFECTS_GLOBAL` work (screen fade alpha, heat-haze, sun-direction accumulation — see §5).

6. **PostDraw / UI / debug / final copy** — `CRenderEngine::PostDraw`, UI render block, `QuickDrawController::Draw`, then `CopySurfaceToTexture` (the present-target blit).

### 3.1 The HDR->LDR composite

Step 7 above — `CToneMappingEffect::DrawHistogramWindow` inside `CRenderBlockPostEffects::Draw` — is the pass that applies tonemapping/exposure to convert the R11G11B10F HDR MainColor into the LDR back-buffer-linear target. (The histogram for auto-exposure is generated in step 1 of the same block.)

### 3.2 Ping-pong slot scheme (why per-eye intermediate captures alias)

`CRenderBlockPostEffects::Draw` threads a single integer "current result-texture slot" through the effects: it reads the current slot index, and each of DoF/MotionBlur/PlayerDamage/AntiAliasing returns the new slot index which is written back, so the chain hops between the three temp textures. The temp arrays are `m_FullscreenSrgbTempTexture[3]` / `m_FullscreenLinearTempTexture[3]` with `m_RenderSetups[3]` (count `s_FullscreenTempTextureCount = 3`). The rotation idiom is: render into `m_RenderSetups[(current+1)%3]`, sample from temp `[current]`, then `current = (current+1)%3`. The "+83" convention is the result-texture-index offset each effect exposes via `GetResultTexture`; effects publish their output texture pointer into the next consumer's input slot.

Because the slot index advances per *dispatch* (and the `%3` ring `render_frame_counters.m_RingIndex` advances per *real frame*), two dispatches in one real frame share the same `%3` ring value but advance the intra-chain slot index twice — so a per-eye capture taken "from slot N" can alias the other eye's intermediate. For VR captures, snapshot from the final composite/back-buffer copy (`CopySurfaceToTexture` output) rather than from an intermediate temp slot.

---

## 4. Buffers & how the frame comes together

Render targets (created in `CreateRenderSetups`):

| Buffer | Name string | Format | Role |
|---|---|---|---|
| MainDepth | `"MainDepthBuffer"` | `SURFACEFORMAT_D32FS8` | scene depth+stencil (reverse-Z) |
| MainColor | `"MainColorBuffer"` | `SURFACEFORMAT_R11G11B10F` | **HDR** scene color |
| GBuffer0 | `"GBuffer0"` | `SURFACEFORMAT_ABGR32` | albedo/material; sRGB+linear aliases |
| GBuffer1 | `"GBuffer1"` | `SURFACEFORMAT_A2R10G10B10` **[UNVERIFIED]** | normals |
| GBuffer2 | `"GBuffer2"` | `SURFACEFORMAT_ABGR32` | material; sRGB alias |
| GBuffer3 | `"GBuffer3"` | `SURFACEFORMAT_ABGR32` | misc/material |
| DownsampledDepth | `"DownsampledDepth"` | half-res depth (D32FS8 + D32F alias) | SSAO/SSR/soft particles |
| Velocity | `"motion_blur_velocity_buffer"` | `SURFACEFORMAT_ABGR32` | screen-space velocity (written in `RP_Z_AND_VELOCITY_PASS`) |
| BackBufferLinear | `"BackBufferLinear"` | ABGR32 (alias of BACK_BUFFER) | final LDR present target (`m_BackBufferLinear`; used as `m_BackBufferRenderSetup` color) |

These RT pointers live on the `GraphicsEngine` singleton (`m_GBufferTexture[4]`, `m_VelocityBufferTexture`, `m_MainDepthTexture`, `m_MainColorBuffer`, `m_DownSampledDepthTexture`, `m_BackBufferLinear`). Reflection-proxy RTs created alongside: `reflection_proxy_water_plane_texture`, `reflection_proxy_depth_texture`, `reflection_proxy_normal_gloss_texture`, `ao_volume_texture`, and 5x `VfxDepthCopy_%d` (the EffectInfo depth-copy slots, §5.1). **[Formats UNVERIFIED.]**

Frame assembly path:
```
GBuffer fill (MainDepth + GBuffer0..3 + Velocity)
  -> lighting/SSR/GI/deferred lights resolve into MainColor (HDR, R11G11B10F)
  -> environment/water/transparency composited into MainColor
  -> post chain (blur/glare/DoF/motionblur), tonemap HDR->LDR (DrawHistogramWindow),
     AA, damage, sun halo, fade  -> intermediate fullscreen temp textures (3-slot ring)
  -> global filters (screen fade, heat haze) -> RP_POSTEFFECTS_GLOBAL
  -> PostDraw + UI + debug
  -> CopySurfaceToTexture -> presentable surface (BackBufferLinear)
  -> graphics_flip (present; suppressed by BLOCK_FLIP in VR)
```

---

## 5. Per-frame-once / single-use state (double-dispatch hazards)

For the double-dispatch VR hack, these advance/consume exactly once per dispatch and so double-step if `HandleDrawThreadTask` (or `Game::Draw`) runs twice per presented frame.

### 5.1 EffectInfo reflection-proxy history (slot machine, `HandleDrawThreadTask+0x267`..`+0x341`)

`m_EffectInfo` = 5 `EffectInfo` slots on the `GraphicsEngine`, stride 80 bytes; `m_EffectInfoIndex` is the current-index int. Each slot stores an `m_FrameIndex` state byte, a depth texture (`m_DepthTexture`, a `VfxDepthCopy_%d`), and an `m_Transform` VP matrix. Per dispatch:

- Loop the 5 slots: state `0` -> remember as free slot; state `2` -> remember; state `3` -> write its index into `m_EffectInfoIndex` (current read slot); state `1` -> increment to `2`.
- If a free slot found: `CopySurfaceToTexture(slot.m_DepthTexture, m_ReflectionProxyDepthSurface)` (capture this frame's proxy depth) and set its state to `1`.
- If a state-2 slot found: promote to `3`.
- `memcpy(slot[current].m_Transform, RenderCamera.m_ViewProjectionF, 0x40)` then set `slot[current].m_FrameIndex = 0`.

This is an N-frame depth+VP history ring for reflection-proxy / reprojection. **Double dispatch:** every slot's state advances twice, so captures age twice as fast; the state-0 slot written by eye 0 is re-aged past its 0->1 dwell by eye 1; and the VP `memcpy` overwrites the same current slot with the same-frame VP. Net: the stored VP no longer matches the depth in the slot -> reflection-proxy ghosting. **Mitigation:** snapshot the 5 `m_FrameIndex` state bytes plus `m_EffectInfoIndex` before eye 0 and restore before eye 1.

### 5.2 dt-driven accumulators in `ApplyWorldFilters` / `ApplyGlobalFilters`

- `CPostEffectsManager::ApplyWorldFilters(dt)`: `dt` flows only into `CPostEffectsManager::ApplyWorldFadeFilter(dt)` — the world fade accumulator. The rest is just texture/setup wiring.
- `CPostEffectsManager::ApplyGlobalFilters(dt)`: advances the screen fade alpha at `this+149` (`+= / -= (1/m_133)*dt`, clamped [0,1]) per dispatch; advances a sun-direction / heat-haze accumulator at `this+327`; calls the DoF update hook. These step twice on double dispatch.

**Mitigation (PLAN §5.3):** set the render context `m_Dt = 0` for the second eye, so dt-driven accumulators do not advance twice. (Auto-exposure is a *separate*, frame-counted hazard that `m_Dt = 0` does **not** touch — see §5.4.)

### 5.3 Shadow parity (`render_frame_counters.m_FrameIndex & 1`)

Parity advances only in the `GraphicsEngine::Draw` prologue, not per dispatch. Two dispatches in one prologue share the same parity — eye 1 reads the same shadow parity slot eye 0 wrote. Benign but means shadows are not independently double-buffered per eye.

### 5.4 Exposure smoother / clock

- `Clock::Update` ticks once in the prologue (per real frame) — keep it there; the slow-mo hazard is calling it twice (PLAN §5.4: gate to once per real frame).
- Auto-exposure is **frame-counted, not dt-driven** — `m_Dt = 0` does **not** touch it. The stereo darkening (~0.74x) was a **second, un-gated histogram meter run on both eyes**, now **fixed** by gating it to one eye.

  **How it works.** `CToneMappingEffect::Update` runs **once per real frame** (`CPostEffectsManager::UpdateRender`); it sets the auto-exposure target to `key / m_Histogram2.m_HistogramMidPoint` — key over the *raw* scene brightness — then adapts `m_CurrentExposure` toward it. There are **two** histograms, both metered via `PopulateHistogram` → per-bucket occlusion queries over a fixed 320x180 RT (a correct read totals 57,600 pixels):
  - `m_Histogram` (`+0x8`): the **exposure-weighted** meter, run by `GenerateHistogramForFinalScene` per dispatch (fed `m_CurrentExposure` through the `"LuminanceToDepthWithExposure"` shader; that exposure-weighting is the feedback loop). The mod already gates it to eye 0.
  - `m_Histogram2` (`+0x2A8`): the **un-weighted** raw-brightness meter — the target's divisor — run by `DrawHistogramWindow`. Despite the name, that function just calls `PopulateHistogram` with a fixed exposure of `1.0`; it also runs per dispatch.

  **The bug + fix.** `GenerateHistogramForFinalScene` was gated to eye 0, but `DrawHistogramWindow` was **not** — so in stereo `m_Histogram2` got metered on *both* dispatches, its occlusion-query ring inflated and corrupted (total 57,600 → ~265,000, with irregular buckets), its mid-point read ~1.35x high, and the exposure divided by a too-large brightness → the frame darkened ~0.74x. The fix is the missing symmetry: gate `DrawHistogramWindow` on eye 1 too (`hooks::graphics_engine::tone_mapping::draw_histogram_window`, under `config.exposure.gate`), so `m_Histogram2` meters once per real frame. Confirmed by the per-frame `ExposureInternals` trace — with the gate, stereo `divisor`/`exposure` match non-stereo and `hist2`'s total returns to 57,600. (An earlier attempt to gate `GenerateHistogramForFinalScene` harder did nothing precisely because that meter feeds `m_Histogram`, not the divisor; the functions are all in `graphics_engine/tone_mapping.pyxis`.)

### 5.5 Screenshot countdown / profiler / end-draw callbacks

- `engine+136` screenshot frames-until countdown decrements per dispatch (two branches at `HandleDrawThreadTask+0x4A9` and `+0x66D`). Gate to eye 0.
- GPU profiler `EndFrame` fires per dispatch — extra profiler entries (harmless).
- End-draw `sig::Signal` one-shot callbacks (`HandleDrawThreadTask+0x5B2`..`+0x65C`) self-remove after first fire; gate to the last (eye 1) dispatch so they fire once.

### 5.6 Draw-done completion event

At the tail of `HandleDrawThreadTask` (`+0x791`..`+0x799`): `*(BYTE*)(engine+20)=1; SetEvent(*(HANDLE*)(engine+24))` — the draw-done signal that `WaitForCPUDrawToFinish` fences on. It fires on **every** dispatch. If you run two dispatches per frame, eye 0 sets the event and the sim side's next `WaitForCPUDrawToFinish` could be released by eye 0's signal before eye 1 finishes. **Mitigation:** suppress the `SetEvent`/`engine+20=1` on eye 0; let only the final (eye 1) dispatch signal. Also `engine+4784`/`+4788` (CPU draw-time floats) are last-writer-wins — harmless.

### 5.7 Jitter phase / frame-counter ring (TAA)

The TAA jitter offset comes from a 2-phase table `flt_142305360` (4 floats) indexed by `render_frame_counters.m_FrameIndex & 1` (set in the Draw prologue), scaled by `dword_142D3A708` and divided by RT width/height. The phase + the `%3` CB ring (`m_RingIndex`) advance **per real frame in the prologue**, NOT per dispatch — so both eyes in one frame share the same jitter phase and CB indices **if you call only the dispatch body twice** (recommended). If you instead call the whole `GraphicsEngine::Draw` twice, the counter double-steps and the two eyes get opposite jitter phases + mismatched CB slots → TAA history mismatch / flicker. **Per-eye TAA:** the velocity buffer's `m_PreviousViewProjection` is snapshotted only in the sim-path `Camera::UpdateRender`, not per dispatch; for correct per-eye velocity, give each eye its own previous-frame VP (or disable AA mode 3 / motion blur during bring-up — PLAN §8.3). For FSR this is now handled in mod code: the velocity-decode pass re-anchors each eye's vectors at its own previous pose using per-eye VP snapshots (`stereo::VpHistory`; the stereo MV correction in `docs/fsr.md` — the fix for issue #10's per-eye shadow-edge flicker). A shared TAA history RT reprojected with two different cameras in one frame is the primary flicker source — use per-eye history or disable temporal AA initially. **[UNVERIFIED]** exact TAA history RT binding + EffectInfo VP-history struct offset.

The mod currently takes the disable path: `config.stereo.force_smaa_1x` (default on) drops `CAntiAliasingEffect`'s resolve mode (`m_Mode`) from 3 (SMAA T2X) to 2 (SMAA 1x) on stereo, and skips the TAA jitter with it, removing the cross-eye temporal ghost. Restoring per-eye T2X would mean giving each eye its own history ping-pong pair on `CAntiAliasingEffect` (textures `this[15]`/`[16]`, render-setups `this[21]`/`[22]`, indices `this[190]`/`[191]`), allocated to match `CAntiAliasingEffect::CreateRenderTargetResources` — the full allocation recipe (the `SCreate2DTextureParams` values and the `CreateRenderSetup` call, mislabeled `LockVolumeTexture` in the IDB) is in the per-eye-T2X task. Low priority: T2X ghosts in VR head-motion regardless of per-eye history.

---

## 6. Quick injection cheat-sheet (VR)

- **Per-eye view divergence (camera-relative — see §2.4/§2.5):** the geometry is camera-relative, so offset the render camera's **`m_TransformF` translation** along the camera right axis — NOT `m_View`'s translation, which the OffsetVP zeros — then re-derive `m_View = Inverse(m_TransformF)` and rebuild `m_ViewProjection`/`m_ViewProjectionF` (§2.6). Hook `Camera::SetupRenderCamera` on the main render camera (`this == engine+0x170`), or right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. For a per-eye off-axis *projection*, write a standard (non-reverse-Z) `m_Projection` before `SetupRenderCamera` so it reverse-Z's once (§2.7). Writing only `m_View`'s translation moves the per-view CB (post / muzzle flash) but not the world.
- **Per-eye pose (gameplay-correct):** hook `GameCameraManager::PushRenderContext` to write the head pose into the control contexts; the engine then derives a consistent `m_View`. Combine with the render-cam offset for true stereo.
- **Once-per-frame state to protect when double-dispatching:** clock (gate), EffectInfo slot bytes (snapshot/restore), eye-1 `m_Dt=0`, screenshot countdown (gate eye 0), end-draw callbacks (gate eye 1). Shadow parity is shared (benign).

---

## 7. Present / swapchain

- **Present** happens in `graphics_flip`. Verified: it takes the context critical section (the immediate-context mutex — see §8), does GPU-timestamp bookkeeping (a 4-deep `&3` query ring), then issues the actual present via a device vtable call (`(*(a1[4]+64))(a1[4], a1[53].HighPart /*sync interval*/, 0)`). The engine-side `GraphicsEngine::Flip` wraps it with `AcquireThreadOwnership`/`ReleaseThreadOwnership`.
- **Swapchain model:** classic **BitBlt `DXGI_SWAP_EFFECT_DISCARD`** (NOT flip-model), created via `IDXGIFactory::CreateSwapChain`. `DXGI_SWAP_CHAIN_DESC`: `Flags=2` (`ALLOW_MODE_SWITCH`), `BufferUsage=0x70` (RENDER_TARGET_OUTPUT | SHADER_INPUT), `SampleDesc.Count=1`. **`BufferCount = ((createFlags>>8)&3) - 1`**, asserted `>=2` → typically **2 back buffers** (double-buffered). Sync interval = `m_DisplayPresentationInterval` (from `CGraphicsEngine::m_FlipInterval`; 0 = no vsync). The swapchain owns the presentable buffers; the engine wraps back-buffer 0 as a `Graphics::HTexture_t` (RTV+SRV) and exposes it via `GetDeviceSurface(BACK_BUFFER)`.
- The `engine+5824` ring is GPU-profiler frame tracking, not a back-buffer texture ring (§1.3). There is no engine-side back-buffer history ring; presentable buffers live in the DXGI swapchain.
- **VR present suppression:** hooking `graphics_flip` and skipping the inner present vtable call is the correct point (the existing `BLOCK_FLIP` does this). The timestamp/query bookkeeping in the wrapper is safe to keep. Grab eye textures before suppressing.

## 8. D3D11 context model

- **The "master context" (`engine+2616`) is a `Context` (`Graphics::HContext_t` wrapper) around the D3D11 *immediate* context.** At device init, `D3D11CreateDevice` writes the immediate context into `Context::m_Context`; `GetMasterContext` returns it. `HandleDrawThreadTask` fetches it each dispatch (`engine+2616 = GetMasterContext(...)`, `HandleDrawThreadTask+0x95`).
- **Deferred contexts exist** (`CreateDeferredContext`) for command-buffer recording; `FinishCommandList`/`ExecuteCommandList` (`Graphics::EndCommandBuffer`/`ApplyCommandBuffer`) record and replay onto the immediate context. `HandleDrawThreadTask` calls `ApplyCommandBuffer(engine+2616)` early (`HandleDrawThreadTask+0xC4`), then issues GBuffer/lighting/post/UI/resolve **directly on the immediate context**. So the render thread ultimately drives the immediate context.
- **Mutex:** every immediate-context op is guarded by `Context::m_Mutex` (a Win32 `CRITICAL_SECTION`), created at device init. Pattern: `ThreadMutexLock`/`Unlock` (`Graphics::CScopedCriticalSection`). `graphics_flip`, `CopyTexture`, `BeginDraw`, viewport sets all take it.
- **VR implications:** for `CreateTexture`/`CopyResource`/per-eye RT injection, use the **immediate context** (`Context::m_Context`) and **take `Context::m_Mutex`** around any immediate-context call to stay coherent with engine draws. Resource *creation* (`CreateTexture2D` on the `ID3D11Device`) is free-threaded (driver reports concurrent creates) and needs no context lock. `CRenderEngine::PostDraw` itself does not take the mutex — the existing mod's "context mutex" is this `Context::m_Mutex`.

## 9. Viewport / render resolution

- **Viewport is set per render-setup, not from a global.** `Graphics::SetRenderSetup`, when binding RTs, sets the viewport to the **bound color/depth target's own dimensions** (`vp = {0,0,target->m_Width, target->m_Height, 0, 1}; RSSetViewports(...)`). So per-pass viewport = bound RT size. (`Graphics::SetViewport` -> `RSSetViewports` exists for explicit sets.)
- **Render resolution source:** every RT in `CreateRenderSetups` is sized from `device->m_DeviceInfo.m_DisplayWidth` / `m_DisplayHeight` (set in `Graphics::Reset`/`SetDisplayMode`). `m_BackBufferLinear` is sized to the same. Some derived RTs are `>>1` (half-res).
- **No dynamic resolution / render-scale** found — RTs are allocated at full display resolution; no scale multiplier between display size and RT size.
- **Per-eye resolution:** because all RT sizes flow from `m_DisplayWidth/Height` through `CreateRenderSetups`, the clean approach is to **set the device display size to the per-eye render resolution and re-run `CreateRenderSetups`** — viewports then follow automatically (driven by RT size in `SetRenderSetup`). You do **not** need to patch per-pass viewport calls.

## 10. UI / HUD pipeline

- **Global instance:** `GetIUIManager` returns the `CUIManager` singleton — a concrete `CUIManager` (Scaleform GFx) singleton (only one instance), vtable `??_7CUIManager@@6B@`.
- **Resolved vtable slots** (the calls in `HandleDrawThreadTask`):
  - `+0x08` `CUIManager::StartRender` — kicks off the async UI render fragment.
  - `+0x10` `CUIManager::SyncRender` — barrier (wait for UI render thread).
  - `+0x18` `CUIManager::Submit` — locks master ctx, flushes UI draws (`m_RenderHAL->Submit`).
  - `+0x28` `CUIManager::RenderOffScreenTextures` — in-world Scaleform screens (not HUD).
  - `+0x30` `CUIManager::RenderStaticBackGround` — pause/menu background.
  - Mapping: early block `HandleDrawThreadTask+0x1F6` = `+0x28` then `+0x08`; main block `HandleDrawThreadTask+0x4FE` = `Graphics::Clear(ctx,2,...)` then `+0x30`, `+0x10`, `+0x18`, then `ResetContext`.
- **Which RT the HUD draws into:** **directly into `m_BackBufferLinear`** (the linear alias of the DXGI back buffer). `CUIManager::InitPlatformRT` binds Scaleform's `m_RenderBuffer` to the RTV of `m_BackBufferLinear` + `m_MainDepthSurface` DSV; `CUIManager::Render` does `SetRenderTarget(m_RenderBuffer)` -> `HAL::Draw`. The HUD is composited **on top of the final LDR image**, after the scene resolve (§12). There is no separate HUD RT for the main overlay.
- **`HandleDrawThreadTask` runs once per frame, not per eye** — so the main UI block is a single HUD emission per real frame already. (If the VR layer calls only the *dispatch body* twice, you must decide which eye gets the UI block; if you call it per eye, gate UI to one.)
- **World-to-screen (markers / distance labels):** `CUIManager::Convert3DCoords` projects a world point to a HUD pixel: `world·vp`, divide by `|w|`, aspect-correct, map NDC→pixels with `m_ViewWidth`/`m_ViewHeight`, return `w > 0`. **The VP is a parameter**, so feeding a chosen per-eye VP relocates where every projected marker lands. Marker placement + off-screen edge-clamp: `CUIManager::Get2DInfo` → `CUIManager::ClampToScreen`; xref `Get2DInfo` to find each gameplay callsite's VP source. The default VP is the render camera's `m_ViewProjectionF`.
- **VR UI compositing:** for desktop dial-in we render the HUD into our own texture (hook `CUIManager::InitPlatformRT`, substitute our offscreen RTV for the surface RTV it binds) and draw it as an **in-scene quad per eye** in the stereo render — so it shows up in the side-by-side preview and we can tune distance/size/follow-lag without a headset. World markers are reprojected with the live per-eye VP rather than baked into the lagging panel (see `docs/hud.md`). An `XrCompositionLayerQuad` is the sharper endpoint once in-headset. Simpler fallback: gate the `StartRender`/`Submit` pair to one eye and copy the HUD region into a layer.

## 11. Render-block / draw-list lifecycle across dispatches

The per-pass draw is **non-destructive**, but the per-frame list **rotation runs inside `GraphicsEngine::Draw`'s prologue** — so running the whole `GraphicsEngine::Draw` twice (as the mod does) rotates the lists twice and the second eye draws an **empty** buffer. This was the long-standing "eye 1 has no scene geometry" bug (measured: eye 0 = 194 indexed draws, eye 1 = 0; eye 1 still issues its ~53 fullscreen/non-RBI draws + 8 compute dispatches, which is why it isn't pure black).

- **Pass array:** `renderEngine + 32*category + 128` is the fixed array of 157 `CRenderPass*` (created at init), iterated by `DrawRenderPassRange` over pass categories (gbuffer 0x2F-0x55, scene 0x56-0x96, ...). Each `CRenderPass` owns a double-buffered `m_Lists[2]` (two `CRBILists`, 0x10 each) followed by `m_CurrentAddList` (`+0x28`) / `m_CurrentDrawList` (`+0x30`).
- **`CRBILists` layout (from `CRBILists::Add`):** `m_List` (array of 0x20-byte entries), `m_ListSize` (u16 capacity), `m_NumElements` (volatile u32; `Add` does `InterlockedExchangeAdd(m_NumElements,1)`). On overflow (`count >= cap`) `Add` spills to a global overflow list (`render_block_overflow_count` plus its 0x18-byte entries, cap 1024).
- **Population (sim, once/frame):** sim systems append blocks into `m_CurrentAddList` via `CRBILists::Add`, dispatched as worker jobs. `CRenderPass::SetupRenderFrameData` is one such per-batch *build* appender — it does **not** swap (the prior "swap" labelling of this function here and in the mod was wrong; it appends `count` items).
- **Rotation / swap (in EVERY `GraphicsEngine::Draw` prologue):** `GraphicsEngine::Draw` calls `CKeep1000Frames::CKeep1000Frames` at `Draw+0x1D0` (the NoDenuvo build's inlined `CRenderPass::SetupRenderFrameData` merged with the profiling constructor). It: (1) toggles a global 1-bit parity `current_add_buffer` at `0x142ED7680` (`parity = (parity-1)&1`); (2) loops all 157 passes (via a sub mislabeled `std::map::operator[]`) calling each pass's `CRenderPass::SaveRenderFrameData` (vtable slot 3): `m_CurrentAddList = &m_Lists[parity]`, `m_CurrentDrawList = &m_Lists[(parity-1)&1]`, and **zeroes the new add-list's `m_NumElements`**; (3) flushes the overflow global back into the lists and resets its count.
- **Drawing does NOT consume:** `CRenderPass::DoDraw` (vtable slot 2) loads `m_CurrentDrawList`, draws `min(m_ListSize, m_NumElements)` blocks via a local cursor, and **never writes `m_NumElements`**. A buffer can be redrawn any number of times.
- **Why eye 1 is empty (running the whole `Draw` twice without saving the parity):** eye 0's rotation points every `m_CurrentDrawList` at the sim-populated buffer → 194 draws. Eye 1's rotation toggles parity *back*, re-points `m_CurrentDrawList` to the buffer eye 0's rotation had just zeroed **and zeroes eye 0's buffer in turn** → 0 draws. The geometry is not consumed by drawing; it is wiped by the *second* rotation. Saving/restoring `current_add_buffer` between eyes prevents this.
- **`CRenderEngine::EraseAllDeletedRenderBlocks` (called `HandleDrawThreadTask+0x6B3`):** a *separate* deletion list; does not touch pass draw lists. Benign across dispatches.
- **Sort task (`m_SortTaskProxy`):** each dispatch creates a fresh sort proxy (`HandleDrawThreadTask+0x1B3`) that sorts each `m_CurrentDrawList` in place; the dispatch spinwaits before `EndDraw`. Sorting an already-sorted stable list is idempotent → safe across two dispatches (serialized by the single render thread).

**Two correct fixes:**
1. **Save/restore `current_add_buffer` between eyes** — what the mod does: snapshot the parity before eye 0's `Draw` and restore it before eye 1's `Draw`. Eye 1's `CKeep1000Frames` then toggles it to the same value as eye 0, so `SaveRenderFrameData` sets the same list pointers and zeroes the same add-list. Eye 1 draws from the same populated draw list as eye 0. The function runs on both eyes, so the overflow list is processed and the external render camera is updated on both eyes too — no side effects from skipping.
2. **Run only the dispatch body twice** (`DispatchDraw`/`HandleDrawThreadTask`) under a single `GraphicsEngine::Draw` prologue — the rotation then runs once and both dispatches read the same populated buffer. More invasive; the mod instead runs the whole `Draw` twice and saves/restores the per-dispatch side effects.

Either way, **no geometry snapshot/restore is needed** — the draw is non-destructive; you only have to stop the *second rotation* from wiping the lists.

## 12. Per-eye output routing

`CopySurfaceToTexture` (symbolized `Graphics::ResolveSurface`, called at `HandleDrawThreadTask+0x5A6`) is an **MSAA resolve** (`ID3D11DeviceContext::ResolveSubresource`, or `CopyResource` if non-MSAA) of the **active render-setup color target (final composited scene + HUD) -> the device's presentable BACK_BUFFER surface** (the surface `m_BackBufferLinear` aliases).

**Recommended approach: (b) copy after the resolve, do NOT redirect it.** Let the resolve run normally (keeps the engine's single presentable surface consistent), then `CopyResource` `m_BackBufferLinear`'s texture into your per-eye texture — exactly what the existing mod does in the `PostDraw`-adjacent path. `m_BackBufferLinear` exposes a clean SRV, so it is directly sampleable/copyable. Do the copy on the **immediate context under `Context::m_Mutex`**, ideally right after `HandleDrawThreadTask+0x5A6`, once per eye dispatch. Redirecting the resolve destination per-eye (approach a) fights the device's internal back-buffer-surface state and the alias relationship, and you still need the back buffer valid for the (suppressed) present bookkeeping.

---

## 13. To dispatch one correct stereo eye — checklist

Two viable structures (§11): **(A)** run the whole `GraphicsEngine::Draw` twice and gate/snapshot the per-frame + per-dispatch side effects (what the mod does), or **(B)** run only the **dispatch body** twice under a single prologue. The lists below assume **(A)**; under **(B)** the prologue items (frame counter, jitter phase, CB ring, clock, **list rotation**) run once and need no gating.

**SET (per eye), on the render camera `engine+368` — hook `Camera::SetupRenderCamera` (`this == engine+0x170`) or right after the active->render memcpy in `GraphicsEngine::TextureCachePlatformUpdate`. The scene is camera-relative (§2.4), so:**
- `m_TransformF` translation: lateral IPD/2 offset along the camera right axis (the camera *world position* — this is what camera-relative geometry subtracts). Do NOT just offset `m_View`'s translation; the OffsetVP zeros it.
- Re-derive `m_View = Inverse(m_TransformF)`, then rebuild `m_ViewProjection`/`m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.
- Per-eye off-axis *projection* (optional): write a standard (non-reverse-Z) `m_Projection` before `SetupRenderCamera` so it reverse-Z's + jitters once (§2.7).
- After the dispatch: `WaitForCPUDrawToFinish`, then `CopyResource(eyeN_tex, m_BackBufferLinear)` under `Context::m_Mutex` (§12).

**SNAPSHOT / RESTORE around the eye pair (so once-per-dispatch state doesn't double-step):**
- EffectInfo slot bytes (`m_EffectInfo[0..4].m_FrameIndex`) + `m_EffectInfoIndex` (§5.1).
- Per-eye previous-frame VP for velocity if you keep motion blur / TAA mode 3 (§5.7); else disable them.
- (Optional) restore the sort proxy state to skip the eye-1 re-sort (perf only, §11).

**GATE to a single eye:**
- **Render-list parity `current_add_buffer` (in the `Draw` prologue) -> save/restore between eyes** — eye 1's `CKeep1000Frames` must toggle to the same value as eye 0's so `SaveRenderFrameData` sets the same list pointers. Without this, eye 1's rotation flips every `m_CurrentDrawList` to the buffer eye 0 just zeroed and draws no scene geometry (§11). This is the core geometry fix under structure (A); not needed under (B).
- Draw-done `SetEvent`/`engine+20=1` (`HandleDrawThreadTask+0x791`) -> **eye 1 (last) only** (§5.6).
- Screenshot countdown `engine+136` -> eye 0 only (§5.5).
- End-draw `sig::Signal` one-shot callbacks (`HandleDrawThreadTask+0x5B2`) -> eye 1 only (§5.5).
- UI block (`HandleDrawThreadTask+0x4FE`) -> one eye (or redirect to a UI layer RT, §10).
- Eye-1 render-context `m_Dt = 0` so dt accumulators (fade/exposure/heat-haze) don't double-step (§5.2).

**LEAVE ALONE (do not touch / shared is fine):**
- Draw-list *contents* — non-destructively drawn, so no re-population/snapshot needed (§11). The per-frame *rotation* runs on both eyes but the parity is saved/restored so both eyes see the same list pointers under structure (A).
- Shadow parity (`render_frame_counters.m_FrameIndex & 1`) — shared slot, benign (§5.3).
- `Clock::Update` — runs once in the sim prologue; do not call per dispatch (§5.4).
- Frame counter / jitter phase / `%3` CB ring — advanced once in the prologue; keep both eyes on the same values (§5.7).
- `graphics_flip` — suppressed once per frame via `BLOCK_FLIP`; present via OpenXR.

---

## Open questions / unverified

1. **GBuffer1/2/3 and aux-RT exact formats** beyond GBuffer0 (`ABGR32`): GBuffer1 as `A2R10G10B10` is unverified (out of scope per the brief — best-effort).
2. **Exposure adaptation EMA** exact location/decay (inferred to be in the tonemapping effect; not line-traced). Neutralized by eye-1 `m_Dt=0` regardless.
3. **`unk_142E5B664` / `unk_142E5B670` consumers** (the `%3` ring values): not traced; presumed triple-buffered per-frame resources. Advanced once/frame in the prologue, so benign for double-dispatch.
4. **TAA history RT binding + EffectInfo VP-history struct offset** (§5.7): the camera-side snapshot chain is verified; the downstream history RT names were not traced.
5. **Reflection-proxy RT formats / `VfxDepthCopy_%d` count**: names confirmed, formats not.
6. **Camera-relative per-object combine site (§2.4)**: the mechanism is established from `SetRenderContextCamera` storing a translation-free OffsetVP (`CalculateOffsetViewProjectionMatrix`) alongside a separate `CameraPosition` (from `m_TransformF`'s translation) — there is no other reason to keep both. The literal per-object `world.row3 -= CameraPosition` multiply in the model render block was not line-verified (a claimed address was a misattribution); not needed for the camera-side fix.
