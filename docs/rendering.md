# JC3 (Apex engine) per-frame rendering pipeline — authoritative reference

Reverse-engineered from the 2025 Denuvo-less Steam build of Just Cause 3 (Apex engine). All addresses are image-base (relative to `0x140000000`); relocate to the runtime base. Where a claim could not be confirmed it is flagged **[UNVERIFIED]**.

Data layout (struct/field offsets, singletons, globals) is byte-stable across the Denuvo / Denuvoless / debug builds — only `.text` (function addresses) moved. So all offsets here are valid on every build; only the function addresses are build-specific.

Key singletons (this build):
- `CGraphicsEngine` instance: `qword_142ED0E18` (= `qword_142E2B6F0`, same pointer; both aliases appear).
- `CCameraManager` (engine-side) instance: `unk_142ED0E20`.
- `CGameCameraManager` instance: a distinct singleton (`Base::CSingle<CGameCameraManager>::Instance`), separate from the engine-side `CCameraManager` (`unk_142ED0E20`) it drives.
- `Base::CClock` instance: `unk_142ED0E78`.

---

## 1. Threading & frame structure

### 1.1 Two threads

- Main / sim thread runs `CGame::Update` -> `CGame::UpdateRender` -> `CGame::Draw` (`0x140CADC30`) -> `CGraphicsEngine::Draw` (`0x1400F4170`).
- Render / draw worker thread runs `CGraphicsEngine::HandleDrawThreadTask` (`0x1400F1D10`), which builds and submits the actual GPU command stream for the frame.

The split is decided in `CGraphicsEngine::DispatchDraw` (`0x1400F3A30`):

```c
if ( CpuPrimaryCount() <= 1 )
    return HandleDrawThreadTask(a1, &unk_142ED0F68);          // inline, same thread
// else: post a fragment job "CallProxy" to the worker pool
v2 = HashString("CallProxy");                                 // cached in unk_142ED0E10
CpuFragmentCall(v2, v4 /* {fn=sub_1400F2C70, ctx, signal} */, 24, &v5, 0);
```

So with >1 CPU primary, `DispatchDraw` enqueues a `CpuFragmentCall` that eventually calls `sub_1400F2C70` -> `HandleDrawThreadTask` on a worker; the completion signal is `a1+48`. With a single primary it runs inline. The work performed is identical either way — only the thread differs.

### 1.2 What `WaitForCPUDrawToFinish` drains (`0x1400C4690`)

```c
if ( *(a1+32) ) CpuFragmentWaitUntilSignalIsNonZero();   // wait on the shared_ptr<SCallProxyKeepAlive> end-draw signal
if ( !*(BYTE*)(a1+20) )                                   // a1+20 = "draw done" event-set flag
    WaitForSingleObject(*(HANDLE*)(a1+24), INFINITE);     // wait on the draw-done Win32 event
```

It blocks until the previous dispatched `HandleDrawThreadTask` has finished (the end-of-draw `SetEvent(*(a1+24))` at the tail of `HandleDrawThreadTask`, `0x1400F24A9`, sets `a1+20=1`). It is the fence between the render worker and the next frame's sim work. `CGame::Draw` calls it at entry and `CGraphicsEngine::Draw` re-checks the same two conditions in its prologue (`0x1400F41A1`/`0x1400F41AF`).

### 1.3 Prologue of `CGraphicsEngine::Draw` (`0x1400F4170`) — per-real-frame-once state

Executed once per real frame, in order:

| Addr | Operation | Notes |
|---|---|---|
| `0x1400F41A1` | wait on `a1+32` fragment signal + `a1+24` event | drain previous frame |
| `0x1400F41C4` | `*(float*)(a1+2736) = dt` | store the frame dt for the worker |
| `0x1400F41D6` | `dword_142D3A6B0 = dword_142D3A6AC++` | **RenderFrameCounter / parity source**: `6B0` = previous value of `6AC`, then `6AC` increments |
| `0x1400F41FC` | `unk_142E5B664 = dword_142D3A6AC % 3` | a `%3` ring index |
| `0x1400F4209` | `unk_142E5B670 = dword_142D3A6B4` | save previous |
| `0x1400F4224` | `dword_142D3A6B4 = dword_142D3A6B0 % 3` | the per-frame `%3` ring used by RBI/CB indices |
| `0x1400F4232` | `++*(engine+5824); if (>= *(engine+5828)) =0` | a ring index at engine `+5824` (count `+5828`). **NOT a back-buffer ring** — this feeds the GPU-profiler frame-query ring (`+5832` consumed by `CGPUProfiler::EndFrame`). The real presentable surfaces are owned by the DXGI swapchain; see §7.1. |
| `0x1400F4254` | `TextureCachePreUpdate` | texture streaming bookkeeping |
| `0x1400F4280` | `CGameStateRun::UpdatePostEffectEdit` | post-effect-edit (dev) sync |
| `0x1400F428E` | `TextureCachePlatformUpdate(engine, masterCtx)` | **the camera copy + render-camera setup happens here — see §2** |
| `0x1400F438D` | `Base::CClock::Update(unk_142ED0E78)` | **the per-frame clock tick** (the slow-mo hazard, see PLAN §5.4) |
| `0x1400F4399` | `CConstantBufferPool::HandBackBuffers(*(a1+4936))` | CB pool ring rotation |
| `0x1400F43AF` | `CRenderEngine::CalculateConstantBufferIndices` | per-RBI CB draw indices |
| `0x1400F43B7` | `DispatchDraw(a1)` | hand the frame to the worker |

The present (Flip) is in this prologue. At `0x1400F4366`-`0x1400F4377`: `if (!*(BYTE*)(a1+4799)) CGraphicsEngine(a1); *(BYTE*)(a1+4799) = 0;` — the sub at `0x1400B89D0` (IDB name `CGraphicsEngine__Flip`) does `AcquireThreadOwnership; Graphics::Flip(device); ReleaseThreadOwnership` (verified: it calls `Graphics::Flip` `0x14195A820` at `0x1400B8A22`). So the prologue presents the previous frame's back buffer, guarded one-shot by the `engine+4799` flag (set elsewhere to skip the very first frame). This is the pipelined-present model PLAN.md describes: Flip-previous, then `DispatchDraw` the current frame. The VR `BLOCK_FLIP` must suppress `Graphics::Flip` (`0x14195A820`) — hooking it directly is correct since this prologue calls it through `0x1400B89D0`.

### 1.4 Body of `HandleDrawThreadTask` (`0x1400F1D10`) — per-dispatch

High-level order (offsets are absolute addresses within the function):

1. `0x1400F1D87` `QueryPerformanceCounter` -> `unk_142ED0F58` (draw start time).
2. `0x1400F1DA5` get master context -> `engine+2616`.
3. `0x1400F1DC8` `CShadowManager::CommitRenderPassSettings` (if `unk_142ED75BB`, i.e. shadows enabled).
4. `0x1400F1E05` `TextureCacheGpuUpdate`.
5. `0x1400F1E3A` `CRenderPass::SetupRenderContext(engine+1824, 0, ctx)` — `engine+1824` is the main `RenderContext`.
6. `0x1400F1E63` `CRenderPass::SetRenderContextCamera(engine+1824, *(unk_142ED0E20+1480))` — feeds the render camera (see §2) into the render context.
7. `0x1400F1E76` `CRenderEngine::SetGlobalShaderConstants(engine, engine+1824)` — uploads per-view CB.
8. `0x1400F1E89` `SetAllGlobalShaderProgramConstants`.
9. If `engine+4798` (m_DoMainDraw) set:
   - UI static-background / sync (`GetIUIManager` vtable +40, +8).
   - `0x1400F1F44` `PreDraw`.
   - `0x1400F1F69` `DrawGBuffer` (passes `0x2F..0x55`).
   - `0x1400F1F77`-`0x1400F2051` **EffectInfo slot machine** (see §5.1).
   - `0x1400F206D` `CRenderEngine::Draw` (passes `0x56..0x95`: lighting/SSR/reflection/main).
   - `0x1400F20CA` `CPostEffectsManager::ApplyWorldFilters(v6, dt, ...)`.
   - `0x1400F20E2` `CRenderEngine::DrawPosteffects` (pass `0x96`).
   - else (no main draw): just UI submit.
10. `0x1400F2185` `CPostEffectsManager::ApplyGlobalFilters(v6, dt, ctx)`.
11. `0x1400F2196` `CRenderEngine::PostDraw`.
12. `0x1400F21B9`/`0x1400F237D` GPU-profiler `EndFrame` (one of two countdown branches), screenshot countdown `engine+136`.
13. `0x1400F220E` UI render block (clear + IUIManager vtable +48/+16/+24).
14. `0x1400F228C` `QuickDrawController::Draw` (debug overlays).
15. `0x1400F22B6` `Graphics::CopySurfaceToTexture(engine+2616)` — final composite copy to the presentable surface (this is the engine-internal "blit to back buffer" the VR redirect intercepts).
16. `0x1400F22C2`-`0x1400F236C` end-draw one-shot signal callbacks (`sig::Signal`) — self-removing.
17. `0x1400F23C3` `EraseAllDeletedRenderBlocks`.
18. `0x1400F2434`/`0x1400F2499` compute draw-time floats (`engine+4784`, `engine+4788`).
19. `0x1400F24A1`-`0x1400F24A9` `*(engine+20)=1; SetEvent(*(engine+24))` — signal draw-done.

### 1.5 Per-frame vs per-dispatch

- **Per real frame (once), in `CGraphicsEngine::Draw` prologue:** frame counter/parity (`6AC`/`6B0`/`6B4`), `%3` ring (`E5B664`/`6B4%3`), back-buffer ring (`engine+5824`), texture-cache pre/platform update, the active->render camera copy, `CClock::Update`, CB pool `HandBackBuffers`, CB indices.
- **Per dispatch (would run twice if you Draw twice), in `HandleDrawThreadTask`:** the entire render pass chain, the EffectInfo slot machine (§5.1), `ApplyWorldFilters`/`ApplyGlobalFilters` accumulators, screenshot countdown, profiler frame begin/end, the final copy + end-draw callbacks.

---

## 2. Camera data flow

### 2.1 Cast of cameras

- `CGameCameraManager` holds `m_ControlContext` with four sub-contexts (Previous/Next x Camera/Render); these carry `m_CameraTransform` (a world transform, not a view matrix), FOV, blur factors.
- `CCameraManager` (engine, `unk_142ED0E20`) holds:
  - `m_ActiveCamera` at `+1472` (`= +0x5C0`): the active scene camera (`NGraphicsEngine::CCamera`, 0x5B0 bytes).
  - `m_RenderCamera` at `+1480` (`= +0x5C8`): the camera the render thread reads.
- A `CCamera` (offsets in bytes): `m_PreviousTransformF +0x14`, `m_TransformF +0x54`, `m_TransformT0 +0x94`, `m_TransformT1 +0xD4`, `m_ProjectionF +0x154`, `m_ViewProjectionF +0x194`, `m_Projection +0x294`, `m_View +0x2D4`, `m_ViewProjection +0x314`. Flag byte at `+1374` (`0x55E`): bit `0x08` = compute-view-from-transform, bit `0x10` = dirty-projection, bit `0x20` = the SetupRenderCamera one-shot guard.

### 2.2 Sim path: where `m_ActiveCamera.m_View` is produced (once per frame)

In `CGameCameraManager::UpdateRender`, runs in the sim path (under `CGameStateRun::UpdateRender`, before `CGame::Draw`):

```
4840  CCameraTree::UpdateRenderContexts(...)         // populate m_ControlContext contexts
4842  CSpecialCameras::UpdateRender(...)
4843  CGameCameraManager::PushRenderContext(this)     // -> InitTransform: m_ActiveCamera.T0 = T1 = ctx transform
4852  NGraphicsEngine::CCameraManager::UpdateRender(unk_142ED0E20, dt, dtf)
```

`PushRenderContext` (`0x1407ECB00`) eventually calls `CCameraManager::InitTransform` (`0x14009D390`):

```c
v2 = *(CameraManager + 1472);                 // = m_ActiveCamera
memcpy(v2 + 148 /*+0x94 T0*/, mat, 0x40);
memcpy(v2 + 212 /*+0xD4 T1*/, mat, 0x40);     // T0 = T1 = context transform
```

So the gameplay camera's *world transform* lands in `m_ActiveCamera.m_TransformT0 == m_TransformT1`.

Then `CCameraManager::UpdateRender` (`0x14011AB70`) iterates every camera in its list and calls `CCamera::UpdateRender` (`0x140...`). That function is where `m_View` is computed:

```c
21167  m_PreviousView = m_View;  m_PreviousProj = m_Projection;  m_PreviousViewProjection = m_ViewProjection;
21170  m_PreviousTransformF = m_TransformF;
21171  Lerp(&m_TransformF, &m_TransformT0, &m_TransformT1, dtf);     // T0==T1 => constant
21173  if ( flag & 0x08 )                                            // SetComputeView(true)
21175      m_View = m_TransformF;
21176      CMatrix4f::Inverse(&m_View);                              // m_View = inverse(world transform)
...
21223  CMatrix4f::Multiply4x4(&m_View, &m_Projection, &m_ViewProjection);
21226  m_ProjectionF = m_Projection;  m_ViewProjectionF = m_ViewProjection;  ...
```

`SetComputeView(true)` sets flag bit `0x08` (`0x1400DB860`). The active gameplay camera has this set, so `m_View` is re-derived as `Inverse(m_TransformF)` every sim frame, from the T0/T1-Lerped transform. Because `InitTransform` set T0 == T1, the Lerp is a no-op constant and engine pose interpolation is effectively disabled for the active camera (independent of `m_InterpolationOverride`).

`m_View` is computed exactly once per real frame, in the sim path, here. It is NOT recomputed at Draw time.

### 2.3 Draw path: active -> render camera copy

At Draw time the prologue calls `TextureCachePlatformUpdate` (`0x1400C46D0`). Near `0x1400C47D8`:

```c
memcpy((char*)this + 368, *(void**)(unk_142ED0E20 + 1472), 0x5B0);   // render-copy = *m_ActiveCamera (whole CCamera)
CCamera::SetupRenderCamera(this + 368, 1);                           // derive VP, reverse-Z, jitter
*(unk_142ED0E20 + 1480) = this + 368;                                // m_RenderCamera = engine+368
```

So the render camera lives inside the CGraphicsEngine object at `engine+368` and is a byte-for-byte copy of `m_ActiveCamera` (including its `m_View`), made once per frame.

`SetupRenderCamera` (`0x1400B3B80`) is guarded by flag bit `0x20` (`m_IsRenderCamera`) at camera `+1374`. The full decompile (verified):

```c
result = *((u8*)this + 1374);
if ( (result & 0x20) == 0 ) {                 // check-AND-SET: skip entirely if already 0x20
    *((u8*)this + 1374) = result | 0x20;
    // reverse-Z fixup on m_Projection (floats 167..180 = +668..+720, i.e. the +660 matrix rows):
    //   e[2]=e[3]-e[2]; e[6]=e[7]-e[6]; e[10]=e[11]-e[10]; e[14]=e[15]-e[14]   (z' = w - z)
    // same reverse-Z on the secondary/previous projection at +852 (floats 215..228):
    if ( a2 ) {                               // a2 = "apply jitter" (always 1 from the prologue call)
        ApplyJitterTransform(engine, this+660, *(int*)(this+362), *(int*)(this+363));
        ApplyJitterTransform(engine, this+852, *(int*)(this+362), *(int*)(this+363));
    }
    Multiply4x4(this+916 /*m_PreviousView*/, this+852 /*m_PreviousProj*/, this+980 /*m_PreviousVP*/);
    Multiply4x4(this+724 /*m_View*/,         this+660 /*m_Projection*/,   this+788 /*m_ViewProjection*/);
    memcpy(this+340 /*m_PrevProjF? (+0x154 m_ProjectionF)*/, this+660, 0x40);
    memcpy(this+404 /*m_ViewProjectionF (+0x194)*/,          this+788, 0x40);
    memcpy(this+468, this+852, 0x40);          // secondary projection float-shadow
    memcpy(this+596, this+980, 0x40);          // secondary VP float-shadow
}
return result;                                 // if 0x20 already set: NO-OP
```

The guard is check-and-set: the body runs only when bit `0x20` is clear, and sets `0x20` as its first action. The active/source camera (`m_ActiveCamera`) never has `0x20` set — `CCamera::UpdateRender` (the sim-path per-frame update, `0x140109F50`) asserts `m_IsRenderCamera == false`, so the active cam carries `0x20` clear. The Draw-prologue memcpy (`0x1400C47D8`) copies it verbatim, so the render copy (`engine+368`) always lands with `0x20` clear; `SetupRenderCamera` then runs its full body (reverse-Z + jitter + VP rebuild) and sets `0x20` on the copy. `m_ViewProjection` / `m_ViewProjectionF` are therefore rebuilt from `m_View x m_Projection` on every Draw. A second call on the same object (a double dispatch reusing the copy) is a no-op, which constrains hook ordering (§2.5).

- reverse-Z fixup on `m_Projection` (+660) and the secondary projection at +852 (z' = w - z, in place);
- `ApplyJitterTransform` (`0x140173AA0`) on both projections (TAA jitter, only effective at AA mode 3);
- `Multiply4x4(this+724 /*m_View*/, this+660 /*m_Projection*/, this+788 /*m_ViewProjection*/)` — i.e. `m_ViewProjection = m_View x m_Projection` (row-major, see §2.6);
- `Multiply4x4(this+916, this+852, this+980)` — the secondary (previous/jittered) VP;
- memcpy `m_ProjectionF <- m_Projection`, `m_ViewProjectionF <- m_ViewProjection`, and the secondary copies.

`SetupRenderCamera` reads `m_View` (+724/+0x2D4) as an input and never writes it. It does not derive `m_View` from any transform. So whatever `m_View` was in the copied active camera is what the render camera uses. Corollary for VR: a hook that writes the render-cam `m_View` *after* `SetupRenderCamera` has already run (`0x20` set) will leave a stale `m_ViewProjection`/`m_ViewProjectionF`, because the re-call is a no-op and nothing else rebuilds VP on the render cam. You must then rebuild VP yourself (§2.6).

### 2.4 What the scene render actually reads (camera-relative)

In `HandleDrawThreadTask`:

```c
0x1400F1E63  SetRenderContextCamera(engine+1824, *(unk_142ED0E20 + 1480))   // = m_RenderCamera (engine+368)
0x1400F1E76  SetGlobalShaderConstants(engine, engine+1824)
```

The main scene is rendered **camera-relative** (for large-world float precision). The opaque-geometry transform is

```
clip = (objectWorld - CameraPosition) x OffsetViewProjection
```

where `OffsetViewProjection` is the view-projection with the view's **translation row zeroed**, and `CameraPosition` is the camera's **world position** supplied separately. The camera translation lives in the per-object subtraction, not in the matrix.

`SetRenderContextCamera` (`0x140187430`, non-shadow path) reads the live render camera (`engine+368`) and fills the render context (`engine+1824`):

- ctx `+0x18` ViewMatrix <- camera `+0x2D4` `m_View` (`0x1401878DD`)
- ctx `+0x58` ProjectionMatrix <- camera `+0x154` `m_ProjectionF` (`0x14018793C`)
- ctx `+0x218` **CameraPosition** <- camera `+0x84` = the translation row of `m_TransformF` (the camera world position) (`0x1401879F2`)
- ctx `+0x224..+0x268` <- the full `m_TransformF` world-transform rows (camera `+0x44..+0x8C`)
- `CalculateOffsetViewProjectionMatrix` (`0x140136020`) is called twice, current + previous (`0x14018799A`/`0x1401879B4`): it copies `m_View`, **zeros its translation row** (`row3 = {0,0,0,1}`), multiplies by the projection, and writes the translation-free **OffsetViewProjection** into the context. This is the VP opaque geometry actually uses.

`SetGlobalShaderConstants` (`0x140185740`) uploads the **global per-view CB**: `GetRenderCamera` (`0x140185E8E`) then `memcpy(this+6428, RenderCamera+404, 0x40)` (`0x140185E97`) copies the *full*, translation-bearing `m_ViewProjectionF`; the camera-position constants come from `RenderCamera+0x84..0x8C` (= `m_TransformF` translation). This per-view block drives **screen-space / non-geometry** work (post-effects, billboards, the camera-position constant); it is **not** what positions opaque geometry vertices.

**Consequence for VR (the key correction):** on the geometry path, `m_View`'s translation is deleted by the OffsetVP zeroing. A per-eye lateral offset must move the **camera world position** — `m_TransformF`'s translation (`+0x84`), hence `CameraPosition` — not `m_View`'s translation. (Confirmed at runtime: writing only `m_View`'s translation gave two divergent `m_ViewProjectionF`s on the render camera but byte-identical geometry between eyes; only the muzzle flash, which rides the per-view CB, shifted.) See §2.5.

### 2.5 Per-eye view injection

Because the geometry is camera-relative (§2.4), the per-eye offset must move the camera **world position** (`m_TransformF`'s translation), not `m_View`'s translation (which the OffsetVP zeros). It targets the **render camera** (`engine+368`), not `m_ActiveCamera` — the sim recomputes the active camera's `m_View = Inverse(m_TransformF)` (flag 0x08) and the prologue memcpy copies it verbatim, so any per-eye divergence has to be applied to the render-camera copy between dispatches.

**Recipe.** Hook `CCamera::SetupRenderCamera` (`0x1400B3B80`) and act only on the main render camera (`this == GraphicsEngine + 0x170`), or equivalently hook right after the active->render memcpy in `TextureCachePlatformUpdate` (`0x1400C47E2`). Per eye:

1. Offset `m_TransformF`'s translation (`+0x84`/`+0x88`/`+0x8C`) by +/- IPD/2 along the camera's right axis (the first basis row of `m_TransformF`). This moves `CameraPosition`, so the camera-relative geometry diverges.
2. Re-derive `m_View = Inverse(m_TransformF)` so the view stays consistent with the moved camera (the OffsetVP's rotation comes from `m_View`; the per-view CB's `m_ViewProjectionF` uses the full `m_View`).
3. Rebuild `m_ViewProjection` / `m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.

Pitfalls:

- Offsetting only `m_View`'s translation does nothing to geometry — the OffsetVP zeros it (§2.4). It shifts only the per-view CB (post / camera-position constant), which is the muzzle-flash-moves-but-world-doesn't symptom.
- Offsetting only `m_TransformF` without re-deriving `m_View` desyncs `CameraPosition` from the OffsetVP rotation and the per-view CB.

### 2.6 The VP-rebuild recipe

The engine's `Matrix4` (`CMatrix4f`) is D3D-style — row-major, row-vector (`clip = p · M`) — and `Multiply4x4(a, b, dest)` writes `dest = a · b`; the full convention (basis-in-rows, the glam bridge) is documented on the `Matrix4` def. `SetupRenderCamera` calls `Multiply4x4(m_View, m_Projection, m_ViewProjection)` ⇒ `m_ViewProjection = m_View · m_Projection`, `clip = p · View · Projection`, and `CCamera::UpdateRender` uses the identical order. So to rebuild a render camera after writing a custom `m_View` and/or `m_Projection`:

```c
CCamera *cam = engine + 368;                                  // == *(unk_142ED0E20 + 1480)
// (write cam->m_View and/or cam->m_Projection first)
Multiply4x4(&cam->m_View, &cam->m_Projection, &cam->m_ViewProjection);  // +0x2D4, +0x294 -> +0x314
memcpy(&cam->m_ProjectionF,     &cam->m_Projection,     0x40);          // +0x154 <- +0x294
memcpy(&cam->m_ViewProjectionF, &cam->m_ViewProjection, 0x40);          // +0x194 <- +0x314
```

The `*F` float-shadow copies (`m_ProjectionF` +0x154, `m_ViewProjectionF` +0x194) are what the render context and global CB actually read (§2.4) — you MUST update them too, not just `m_ViewProjection`.

### 2.7 Reverse-Z / depth convention + the per-eye projection wedge bug

The base projection built by `RecalcProjection` (`0x140013347`, via `PerspectiveFov` / `PerspectiveOffCenter` / `Ortho`) is a standard (non-reversed) projection. The reverse-Z remap `z' = w - z` (`e[2]=e[3]-e[2]; e[6]=e[7]-e[6]; e[10]=e[11]-e[10]; e[14]=e[15]-e[14]`) is applied exactly once per render camera, gated by bit `0x20`:

- `RecalcProjection` applies it only `if (flags & 0x20)` (`v49 = (flags & 0x20)==0; if (!v49) { e[2]=e[3]-e[2]; ... }`) — i.e. only when called on a render cam.
- `SetupRenderCamera` applies it **unconditionally to whatever is in `m_Projection`** when its `0x20`-guard body runs (§2.3). There is no "already reversed?" check.

Result: reverse-Z is applied once, by whichever of {RecalcProjection-on-render-cam, SetupRenderCamera} first sees `0x20` clear. Depth convention: reverse-Z, NDC z in [0,1] with near=1, far=0 (MainDepth is `D32FS8`), enabling far-plane precision / infinite-far style projections.

**The wedge bug (VR):** if you write a per-eye off-axis projection into the render cam's `m_Projection` **before** `SetupRenderCamera` runs, `SetupRenderCamera` will reverse-Z it. So:

- **Preferred:** supply a standard (non-reversed) off-axis projection into `m_Projection` *before* `SetupRenderCamera`, and let the engine apply reverse-Z + jitter once. Matches engine convention; you also get TAA jitter for free.
- **Alternative:** supply an already-reverse-Z'd projection *after* `SetupRenderCamera` (when `0x20` is set, so it won't re-reverse), then rebuild VP/`*F` yourself (§2.6). With this path you must apply jitter yourself if AA mode 3 is active, and maintain `m_PreviousProj`/`m_PreviousViewProjection` for velocity. Do not feed an already-reversed projection into the pre-`SetupRenderCamera` window — that double-applies the remap (the wedge artifact).

`ApplyJitterTransform` (`0x140173AA0` -> `CAntiAliasingEffect::ApplySubsampleJitter` `0x1400C7700`) only fires when AA mode `== 3`; it post-multiplies a sub-pixel clip-space translation (`m[12]=jx`, `m[13]=jy`, `j = ±offset/width,height`) onto the projection: `proj = proj · jitterMat`.

---

## 3. Render passes / stages (ordered)

The pass system is a flat enum `ERenderPass` of ~180 `RP_*` values (ordered, contiguous; names recovered from the pass-name switch). Passes are drawn by index range via `CRenderEngine::DrawRenderPassRange(ctx, renderSetup, first, last)` (`0x140186600`), which walks `CRenderEngine::m_RenderPasses[]` (a fixed array of 157 `CRenderPass*` at `renderEngine + 32*pass + 128`, created at init) and vtable-dispatches each pass's render blocks. The per-frame render-block instances live in each `CRenderPass`'s double-buffered `m_Lists[2]` (`CRBILists`); see §11.

Ordered stages per dispatch:

1. **Shadow atlas** — committed in `CShadowManager::CommitRenderPassSettings` (`0x1400F1DC8`) and rendered as the `RP_SHADOW_0..7` / `RP_STATIC_SHADOW_0..7` / `RP_SHADOW_REFLECTIVE_*` passes. Shadow data is **parity-buffered** by `dword_142D3A6B0 & 1`: `SetRenderContextCamera` and the shadow-matrix reads index shadow storage by `(dword_142D3A6B0 & 1) << 8` (a 256-byte stride per parity) at `0x14018751A`, `0x1401875F3`, `0x140187B56`. So shadows ping-pong between two parity buffers each real frame. Running Draw twice in one real frame uses the same parity twice (parity only advances in the `CGraphicsEngine::Draw` prologue, `0x1400F41D6`), which is benign for the second eye but means the second eye sees the same parity slot.

2. **GBuffer fill** — `DrawGBuffer` (`0x140186810`) -> `DrawRenderPassRange(setup, 0x2F, 0x55)`. The depth/velocity prefix is `RP_Z_OCCLUDERS`, `RP_Z_COARSE_PASS`, `RP_Z_PASS`, `RP_Z_AND_VELOCITY_PASS` (this is where the velocity buffer is written using the previous-frame VP), then static/dynamic models, then `RP_DECALS` / `RP_SCREEN_SPACE_DECALS` / `RP_SCREEN_SPACE_ROAD_DECALS`, terminating at `RP_LAST_GBUFFER` (0x55). Before drawing it binds two FS textures (slots 0x28, 0x29).

3. **Lighting / reflections / main** — `CRenderEngine::Draw` (`0x1401868A0`) -> `DrawRenderPassRange(setup, 0x56, 0x96)`. In order: `RP_REFLECTIVE_WATER_PLANES`, `RP_AO_VOLUMES`, `RP_SSAO`, `RP_SCREEN_SPACE_REFLECTIONS`, `RP_GLOBAL_ILLUMINATION`, `RP_SCREEN_SPACE_SUBSURFACE_SKIN`, `RP_DEFERRED_LIGHTS` (these resolve lighting into MainColor), then opaque/`RP_LAST_OPAQUE`, environment (`RP_STARS`/`RP_SUN`/`RP_MOON`/`RP_SKYBOX`), water, transparency, ending just before `RP_POSTEFFECTS`. Reflection-proxy passes (`RP_REFLECTION_*`) and `RP_ENVREFLECTION` are also in this block.

4. **Post-effects (world)** — `ApplyWorldFilters` (`0x14014BFE0`) enqueues the world post-effect block, then `DrawPosteffects` (`0x140186910`) -> `DrawRenderPassRange(setup, 0x96, 0x97)` runs pass `0x96` (`RP_POSTEFFECTS`). The actual HDR chain executes inside `CRenderBlockPostEffects::Draw` (`0x14016A260`), in order:
   1. `CToneMappingEffect::GenerateHistogramForFinalScene` (exposure histogram)
   2. `CSunHaloEffect::PreApply`
   3. blur: bokeh path (`CDownScale2x2PackFocus::Apply` -> `CBlurEffectBokeh::Apply`) if `IsBokehActive`, else `CBlurEffect::Apply`
   4. `CGlareEffect::Apply`
   5. `CDepthOfFieldEffect::Apply`
   6. `CMotionBlurEffect::Apply` (gated by motion-blur active / AA-mode==3 / heat-haze)
   7. `CToneMappingEffect::DrawHistogramWindow` (**the HDR->LDR tonemap composite**)
   8. `CPlayerDamageEffect::Apply` (if damage flag set)
   9. `CAntiAliasingEffect::Apply`
   10. `CSunHaloEffect::Apply` + additive sun blend (`SetBlendFunc(5,5,6)`)
   11. `CFadeEffect::Apply` (final fade)

5. **Global filters** — `ApplyGlobalFilters` (`0x14014C0C0`) enqueues `RP_POSTEFFECTS_GLOBAL` work (screen fade alpha, heat-haze, sun-direction accumulation — see §5).

6. **PostDraw / UI / debug / final copy** — `CRenderEngine::PostDraw` (`0x1401C2350`), UI render block, `QuickDrawController::Draw`, then `Graphics::CopySurfaceToTexture` (the present-target blit).

### 3.1 The HDR->LDR composite

Step 7 above — `CToneMappingEffect::DrawHistogramWindow` inside `CRenderBlockPostEffects::Draw` — is the pass that applies tonemapping/exposure to convert the R11G11B10F HDR MainColor into the LDR back-buffer-linear target. (The histogram for auto-exposure is generated in step 1 of the same block.)

### 3.2 Ping-pong slot scheme (why per-eye intermediate captures alias)

`CRenderBlockPostEffects::Draw` threads a single integer "current result-texture slot" through the effects: it reads the current slot index, and each of DoF/MotionBlur/PlayerDamage/AntiAliasing returns the new slot index which is written back, so the chain hops between the three temp textures. The temp arrays are `m_FullscreenSrgbTempTexture[3]` / `m_FullscreenLinearTempTexture[3]` with `m_RenderSetups[3]` (count `s_FullscreenTempTextureCount = 3`). The rotation idiom is: render into `m_RenderSetups[(current+1)%3]`, sample from temp `[current]`, then `current = (current+1)%3`. The "+83" convention is the result-texture-index offset each effect exposes via `GetResultTexture`; effects publish their output texture pointer into the next consumer's input slot.

Because the slot index advances per *dispatch* (and the `%3` ring `dword_142D3A6B4 = ...%3` advances per *real frame*), two dispatches in one real frame share the same `%3` ring value but advance the intra-chain slot index twice — so a per-eye capture taken "from slot N" can alias the other eye's intermediate. For VR captures, snapshot from the final composite/back-buffer copy (`Graphics::CopySurfaceToTexture` output) rather than from an intermediate temp slot.

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

Reflection-proxy RTs created alongside: `reflection_proxy_water_plane_texture`, `reflection_proxy_depth_texture`, `reflection_proxy_normal_gloss_texture`, `ao_volume_texture`, and 5x `VfxDepthCopy_%d` (the EffectInfo depth-copy slots, §5.1). **[Formats UNVERIFIED.]**

Frame assembly path:
```
GBuffer fill (MainDepth + GBuffer0..3 + Velocity)
  -> lighting/SSR/GI/deferred lights resolve into MainColor (HDR, R11G11B10F)
  -> environment/water/transparency composited into MainColor
  -> post chain (blur/glare/DoF/motionblur), tonemap HDR->LDR (DrawHistogramWindow),
     AA, damage, sun halo, fade  -> intermediate fullscreen temp textures (3-slot ring)
  -> global filters (screen fade, heat haze) -> RP_POSTEFFECTS_GLOBAL
  -> PostDraw + UI + debug
  -> Graphics::CopySurfaceToTexture -> presentable surface (BackBufferLinear)
  -> Graphics::Flip (present; suppressed by BLOCK_FLIP in VR)
```

---

## 5. Per-frame-once / single-use state (double-dispatch hazards)

For the double-dispatch VR hack, these advance/consume exactly once per dispatch and so double-step if `HandleDrawThreadTask` (or `CGame::Draw`) runs twice per presented frame.

### 5.1 EffectInfo reflection-proxy history (slot machine, `0x1400F1F77`-`0x1400F2051`)

`m_EffectInfo` = 5 slots, stride 80 bytes, base at `engine+4328`; current-index int at `engine+4808`. Each slot stores a `m_FrameIndex` state byte, a depth texture (`VfxDepthCopy_%d`), and a VP matrix. Per dispatch:

- Loop the 5 slots: state `0` -> remember as free slot `v13`; state `2` -> remember as `v14`; state `3` -> write its index into `engine+4808` (current read slot); state `1` -> increment to `2`.
- If a free slot found: `CopySurfaceToTexture(slot.depth, m_ReflectionProxyDepthSurface)` (capture this frame's proxy depth) and set its state to `1`.
- If a state-2 slot found: promote to `3`.
- `memcpy(slot[current].transform, RenderCamera+404 /*m_ViewProjectionF*/, 0x40)` then set `slot[current].state = 0`.

This is an N-frame depth+VP history ring for reflection-proxy / reprojection. **Double dispatch:** every slot's state advances twice, so captures age twice as fast; the state-0 slot written by eye 0 is re-aged past its 0->1 dwell by eye 1; and the VP `memcpy` overwrites the same current slot with the same-frame VP. Net: the stored VP no longer matches the depth in the slot -> reflection-proxy ghosting. **Mitigation:** snapshot the ~6 state bytes (`engine+4328`+0,80,160,240,320 `m_FrameIndex`) plus the int at `engine+4808` before eye 0 and restore before eye 1.

### 5.2 dt-driven accumulators in `ApplyWorldFilters` / `ApplyGlobalFilters`

- `ApplyWorldFilters(dt)` (`0x14014BFE0`): `dt` flows only into `ApplyWorldFadeFilter(this, dt)` (`0x1400F9BD0`) — the world fade accumulator. The rest is just texture/setup wiring.
- `ApplyGlobalFilters(dt)` (`0x14014C0C0`): advances the screen fade alpha at `this+149` (`+= / -= (1/m_133)*dt`, clamped [0,1]) per dispatch; advances a sun-direction / heat-haze accumulator at `this+327`; calls the DoF update hook. These step twice on double dispatch.

**Mitigation (PLAN §5.3):** set the render context `m_Dt = 0` for the second eye, so dt-driven accumulators do not advance twice. (Auto-exposure is a *separate*, frame-counted hazard that `m_Dt = 0` does **not** touch — see §5.4.)

### 5.3 Shadow parity (`dword_142D3A6B0 & 1`)

Parity advances only in the `CGraphicsEngine::Draw` prologue (`0x1400F41D6`), not per dispatch. Two dispatches in one prologue share the same parity — eye 1 reads the same shadow parity slot eye 0 wrote. Benign but means shadows are not independently double-buffered per eye.

### 5.4 Exposure smoother / clock

- `Base::CClock::Update` (`0x140093230`) ticks once in the prologue (per real frame) — keep it there; the slow-mo hazard is calling it twice (PLAN §5.4: gate to once per real frame).
- Auto-exposure is **frame-counted, not dt-driven** — `m_Dt = 0` does **not** fix it (dt feeds only motion blur). Two stages: the *adaptation* (`CToneMappingEffect::Update`, run per real frame from `CPostEffectsManager::UpdateRender`) reads the histogram bright-point and steps the exposure EMA; the *histogram population* (`CToneMappingEffect::Apply` + `GenerateHistogramForFinalScene`) runs inside `CRenderBlockPostEffects::Draw` — i.e. **per dispatch**. The mod already gates the adaptation on eye 1 (`GATE_EXPOSURE` → `SSmoothedExposure::Update` + `CalculateMidAndBrightPointForHistogram`), but the population still runs on both eyes, so the once-per-frame readback sees a histogram filled by both eyes and the exposure settles too dark — a visible ramp the moment stereo is enabled. **Fix:** also gate the histogram population on eye 1 (the mod already hooks it, `GENERATE_HISTOGRAM`); then eye 0 both populates and adapts, eye 1 does neither. Alternatively snapshot the `SHistogramGeneration` bucket arrays after eye 0 and restore before eye 1's `Update`. **[Mechanism verified from the dev-build decompile; exact release addresses pending re-resolution — the split-dump is a different build.]**

### 5.5 Screenshot countdown / profiler / end-draw callbacks

- `engine+136` screenshot frames-until countdown decrements per dispatch (two branches at `0x1400F21B9` and `0x1400F237D`). Gate to eye 0.
- GPU profiler `EndFrame` fires per dispatch — extra profiler entries (harmless).
- End-draw `sig::Signal` one-shot callbacks (`0x1400F22C2`-`0x1400F236C`) self-remove after first fire; gate to the last (eye 1) dispatch so they fire once.

### 5.6 Draw-done completion event

At the tail of `HandleDrawThreadTask` (`0x1400F24A1`-`0x1400F24A9`): `*(BYTE*)(engine+20)=1; SetEvent(*(HANDLE*)(engine+24))` — the draw-done signal that `WaitForCPUDrawToFinish` (`0x1400C4690`) fences on. It fires on **every** dispatch. If you run two dispatches per frame, eye 0 sets the event and the sim side's next `WaitForCPUDrawToFinish` could be released by eye 0's signal before eye 1 finishes. **Mitigation:** suppress the `SetEvent`/`engine+20=1` on eye 0; let only the final (eye 1) dispatch signal. Also `engine+4784`/`+4788` (CPU draw-time floats) are last-writer-wins — harmless.

### 5.7 Jitter phase / frame-counter ring (TAA)

The TAA jitter offset comes from a 2-phase table `flt_142305360` (4 floats) indexed by `dword_142D3A6B0 & 1` (the previous-frame counter set in the Draw prologue at `0x1400F41D6`), scaled by `dword_142D3A708` and divided by RT width/height. The phase + the `%3` CB ring (`dword_142D3A6B4`) advance **per real frame in the prologue**, NOT per dispatch — so both eyes in one frame share the same jitter phase and CB indices **if you call only the dispatch body twice** (recommended). If you instead call the whole `CGraphicsEngine::Draw` twice, the counter double-steps and the two eyes get opposite jitter phases + mismatched CB slots → TAA history mismatch / flicker. **Per-eye TAA:** the velocity buffer's `m_PreviousViewProjection` is snapshotted only in the sim-path `CCamera::UpdateRender` (`0x140109F50`), not per dispatch; for correct per-eye velocity, give each eye its own previous-frame VP (or disable AA mode 3 / motion blur during bring-up — PLAN §8.3). A shared TAA history RT reprojected with two different cameras in one frame is the primary flicker source — use per-eye history or disable temporal AA initially. **[UNVERIFIED]** exact TAA history RT binding + EffectInfo VP-history struct offset.

The mod currently takes the disable path: `FORCE_SMAA_1X` (default on) drops `CAntiAliasingEffect`'s resolve mode (`+0x300`) from 3 (SMAA T2X) to 2 (SMAA 1x) on stereo, and skips the TAA jitter with it, removing the cross-eye temporal ghost. Restoring per-eye T2X would mean giving each eye its own history ping-pong pair on `CAntiAliasingEffect` (textures `this[15]`/`[16]`, render-setups `this[21]`/`[22]`, indices `this[190]`/`[191]`), allocated to match `CAntiAliasingEffect::CreateRenderTargetResources` (`0x1400A5E30`) — the full allocation recipe (the `SCreate2DTextureParams` values and the `CreateRenderSetup` call, mislabeled `LockVolumeTexture` `0x1419545F0` in the IDB) is in the per-eye-T2X task. Low priority: T2X ghosts in VR head-motion regardless of per-eye history.

---

## 6. Quick injection cheat-sheet (VR)

- **Per-eye view divergence (camera-relative — see §2.4/§2.5):** the geometry is camera-relative, so offset the render camera's **`m_TransformF` translation (`+0x84`)** along the camera right axis — NOT `m_View`'s translation, which the OffsetVP zeros — then re-derive `m_View = Inverse(m_TransformF)` and rebuild `m_ViewProjection`/`m_ViewProjectionF` (§2.6). Hook `SetupRenderCamera` (`0x1400B3B80`) on the main render camera (`this == engine+0x170`), or right after the active->render memcpy (`0x1400C47E2`). For a per-eye off-axis *projection*, write a standard (non-reverse-Z) `m_Projection` before `SetupRenderCamera` so it reverse-Z's once (§2.7). Writing only `m_View`'s translation moves the per-view CB (post / muzzle flash) but not the world.
- **Per-eye pose (gameplay-correct):** hook `PushRenderContext` (`0x1407ECB00`) to write the head pose into the control contexts; the engine then derives a consistent `m_View`. Combine with the render-cam offset for true stereo.
- **Once-per-frame state to protect when double-dispatching:** clock (gate), EffectInfo slot bytes (snapshot/restore), eye-1 `m_Dt=0`, screenshot countdown (gate eye 0), end-draw callbacks (gate eye 1). Shadow parity is shared (benign).

---

## 7. Present / swapchain

- **Present** happens in `Graphics::Flip` (`0x14195A820`). Verified: it takes the context critical section (`*(device+32808)`, the immediate-context mutex — see §8), does GPU-timestamp bookkeeping (a 4-deep `&3` query ring at `device+128`/`+129`/`+133`), then issues the actual present via a device vtable call (`(*(a1[4]+64))(a1[4], a1[53].HighPart /*sync interval*/, 0)`). The engine-side `CGraphicsEngine::Flip` (`0x1400B89D0`) wraps it with `AcquireThreadOwnership`/`ReleaseThreadOwnership`.
- **Swapchain model:** classic **BitBlt `DXGI_SWAP_EFFECT_DISCARD`** (NOT flip-model), created via `IDXGIFactory::CreateSwapChain`. `DXGI_SWAP_CHAIN_DESC`: `Flags=2` (`ALLOW_MODE_SWITCH`), `BufferUsage=0x70` (RENDER_TARGET_OUTPUT | SHADER_INPUT), `SampleDesc.Count=1`. **`BufferCount = ((createFlags>>8)&3) - 1`**, asserted `>=2` → typically **2 back buffers** (double-buffered). Sync interval = `m_DisplayPresentationInterval` (from `CGraphicsEngine::m_FlipInterval`; 0 = no vsync). The swapchain owns the presentable buffers; the engine wraps back-buffer 0 as a `Graphics::HTexture_t` (RTV+SRV) and exposes it via `GetDeviceSurface(BACK_BUFFER)`.
- The `engine+5824/+5828` ring is GPU-profiler frame tracking, not a back-buffer texture ring (§1.3). There is no engine-side back-buffer history ring; presentable buffers live in the DXGI swapchain.
- **VR present suppression:** hooking `Graphics::Flip` (`0x14195A820`) and skipping the inner present vtable call is the correct point (the existing `BLOCK_FLIP` does this). The timestamp/query bookkeeping in the wrapper is safe to keep. Grab eye textures before suppressing.

## 8. D3D11 context model

- **The "master context" (`engine+2616`) is a `Graphics::HContext_t` wrapper around the D3D11 *immediate* context.** At device init, `D3D11CreateDevice` writes the immediate context into `m_Context->m_Context` (`HContext_t+0x8020`); `Graphics::GetMasterContext` (`0x1419550D0`) returns it. `HandleDrawThreadTask` fetches it each dispatch (`engine+2616 = GetMasterContext(...)`, `0x1400F1DAA`).
- **Deferred contexts exist** (`CreateDeferredContext`) for command-buffer recording; `FinishCommandList`/`ExecuteCommandList` (`Graphics::EndCommandBuffer`/`ApplyCommandBuffer`) record and replay onto the immediate context. `HandleDrawThreadTask` calls `ApplyCommandBuffer(engine+2616)` early (`0x1400F1DD4`), then issues GBuffer/lighting/post/UI/resolve **directly on the immediate context**. So the render thread ultimately drives the immediate context.
- **Mutex:** every immediate-context op is guarded by `m_Context->m_Mutex` (a Win32 `CRITICAL_SECTION`) at **`HContext_t + 0x8028` (32808)**, created at device init. Pattern: `ThreadMutexLock`/`Unlock` (`Graphics::CScopedCriticalSection`). `Graphics::Flip`, `CopyTexture`, `BeginDraw`, viewport sets all take it.
- **VR implications:** for `CreateTexture`/`CopyResource`/per-eye RT injection, use the **immediate context** (`engine+2616 -> m_Context`) and **take `m_Context->m_Mutex` (+0x8028)** around any immediate-context call to stay coherent with engine draws. Resource *creation* (`CreateTexture2D` on the `ID3D11Device`) is free-threaded (driver reports concurrent creates) and needs no context lock. `CRenderEngine::PostDraw` (`0x1401C2350`) itself does not take the mutex — the existing mod's "context mutex" is this `m_Context->m_Mutex` at +0x8028.

## 9. Viewport / render resolution

- **Viewport is set per render-setup, not from a global.** `Graphics::SetRenderSetup`, when binding RTs, sets the viewport to the **bound color/depth target's own dimensions** (`vp = {0,0,target->m_Width, target->m_Height, 0, 1}; RSSetViewports(...)`). So per-pass viewport = bound RT size. (`Graphics::SetViewport` -> `RSSetViewports` exists for explicit sets.)
- **Render resolution source:** every RT in `CreateRenderSetups` is sized from `device->m_DeviceInfo.m_DisplayWidth` / `m_DisplayHeight` (set in `Graphics::Reset`/`SetDisplayMode`). `m_BackBufferLinear` is sized to the same. Some derived RTs are `>>1` (half-res).
- **No dynamic resolution / render-scale** found — RTs are allocated at full display resolution; no scale multiplier between display size and RT size.
- **Per-eye resolution:** because all RT sizes flow from `m_DisplayWidth/Height` through `CreateRenderSetups`, the clean approach is to **set the device display size to the per-eye render resolution and re-run `CreateRenderSetups`** — viewports then follow automatically (driven by RT size in `SetRenderSetup`). You do **not** need to patch per-pass viewport calls.

## 10. UI / HUD pipeline

- **Global instance:** `GetIUIManager` (`0x1400995A0`) returns `0x142E5B620` — a concrete `CUIManager` (Scaleform GFx) singleton (only one instance). vtable `??_7CUIManager@@6B@` @ `0x1424E2778`.
- **Resolved vtable slots** (the calls in `HandleDrawThreadTask`):
  - `+0x08` `CUIManager::StartRender` (`0x140F1B030`) — kicks off the async UI render fragment.
  - `+0x10` `CUIManager::SyncRender` (`0x140F1B0C0`) — barrier (wait for UI render thread).
  - `+0x18` `CUIManager::Submit` (`0x140F1B0D0`) — locks master ctx, flushes UI draws (`m_RenderHAL->Submit`).
  - `+0x28` `CUIManager::RenderOffScreenTextures` (`0x1410076C0`) — in-world Scaleform screens (not HUD).
  - `+0x30` `CUIManager::RenderStaticBackGround` (`0x140F46C20`) — pause/menu background.
  - Mapping: early block `0x1400F1F06` = `+0x28` then `+0x08`; main block `0x1400F220E` = `Graphics::Clear(ctx,2,...)` then `+0x30`, `+0x10`, `+0x18`, then `ResetContext`.
- **Which RT the HUD draws into:** **directly into `m_BackBufferLinear`** (the linear alias of the DXGI back buffer). `CUIManager::InitPlatformRT` binds Scaleform's `m_pDisplayRT` to the RTV of `m_BackBufferLinear` + `m_MainDepthSurface` DSV; `CUIManager::Render` does `SetRenderTarget(m_pDisplayRT)` -> `HAL::Draw`. The HUD is composited **on top of the final LDR image**, after the scene resolve (§12). There is no separate HUD RT for the main overlay.
- **`HandleDrawThreadTask` runs once per frame, not per eye** — so the UI block at `0x1400F220E` is a single HUD emission per real frame already. (If the VR layer calls only the *dispatch body* twice, you must decide which eye gets the UI block; if you call it per eye, gate UI to one.)
- **World-to-screen (markers / distance labels):** `CUIManager::Convert3DCoords` (`0x140F69A70`, verified) projects a world point to a HUD pixel — `bool(this, CVector3f *world, float *outX, float *outY, CMatrix4f *vp)`: `world·vp`, divide by `|w|`, aspect-correct (fields at `this+0x148C`/`+0x14A0`), map NDC→pixels with `viewW`/`viewH` at `this+0x1484`/`+0x1488`, return `w > 0`. **The VP is a parameter**, so feeding a chosen per-eye VP relocates where every projected marker lands. Marker placement + off-screen edge-clamp: `CUIManager::Get2DInfo` (`0x140F69CB0`) → `ClampToScreen` (`0x140F470A0`); xref `Get2DInfo` to find each gameplay callsite's VP source. The default VP is the render camera's `m_ViewProjectionF` (camera at `*(0x142ED0E20+0x5C0)`, `+0x194`).
- **VR UI compositing:** for desktop dial-in we render the HUD into our own texture (hook `CUIManager::InitPlatformRT` `0x140F696E0`, substitute our offscreen RTV for the surface RTV it binds) and draw it as an **in-scene quad per eye** in the stereo render — so it shows up in the side-by-side preview and we can tune distance/size/follow-lag without a headset. World markers are reprojected with the live per-eye VP rather than baked into the lagging panel (see `docs/hud.md`). An `XrCompositionLayerQuad` is the sharper endpoint once in-headset. Simpler fallback: gate the `StartRender`/`Submit` pair (`0x140F1B030`/`0x140F1B0D0`) to one eye and copy the HUD region into a layer.

## 11. Render-block / draw-list lifecycle across dispatches

The per-pass draw is **non-destructive**, but the per-frame list **rotation runs inside `CGraphicsEngine::Draw`'s prologue** — so running the whole `CGraphicsEngine::Draw` twice (as the mod does) rotates the lists twice and the second eye draws an **empty** buffer. This was the long-standing "eye 1 has no scene geometry" bug (measured: eye 0 = 194 indexed draws, eye 1 = 0; eye 1 still issues its ~53 fullscreen/non-RBI draws + 8 compute dispatches, which is why it isn't pure black).

- **Pass array:** `renderEngine + 32*category + 128` is the fixed array of 157 `CRenderPass*` (created at init), iterated by `DrawRenderPassRange` (`0x140186600`) over pass categories (gbuffer 0x2F-0x55, scene 0x56-0x96, ...). Each `CRenderPass` owns a double-buffered `m_Lists[2]` (two `CRBILists`, 0x10 each) followed by `m_CurrentAddList` (`+0x28`) / `m_CurrentDrawList` (`+0x30`).
- **`CRBILists` layout (from `CRBILists::Add` `0x14011C070`):** `+0x0` `m_List` (array of 0x20-byte entries), `+0x8` `m_ListSize` (u16 capacity), `+0xC` `m_NumElements` (volatile u32; `Add` does `InterlockedExchangeAdd(this+0xC,1)`). On overflow (`count >= cap`) `Add` spills to a global overflow list (count at `0x142ED0FA0`, 0x18-byte entries at `0x142ED0FB0`, cap 1024).
- **Population (sim, once/frame):** sim systems append blocks into `m_CurrentAddList` via `CRBILists::Add`, dispatched as worker jobs. `CRenderPass::SetupRenderFrameData` (`0x14048C4E0`) is one such per-batch *build* appender — it does **not** swap (the prior "swap" labelling of this function here and in the mod was wrong; it appends `count` items and derefs `a3+0x8038`).
- **Rotation / swap (in EVERY `CGraphicsEngine::Draw` prologue):** `CGraphicsEngine::Draw` (`0x1400F4170`) calls the list-rotation driver `0x1401A3000` at site `0x1400F4340` (mislabeled `CKeep1000Frames::CKeep1000Frames`). It: (1) toggles a global 1-bit parity `dword_142ED7680` (`parity = (parity-1)&1`); (2) loops all 157 passes (`0x1401A2F60`, mislabeled `std::map::operator[]`) calling each pass's `CRenderPass::SaveRenderFrameData` (`0x140194480`, vtable slot 3): `m_CurrentAddList = &m_Lists[parity]`, `m_CurrentDrawList = &m_Lists[(parity-1)&1]`, and **zeroes the new add-list's `m_NumElements`**; (3) flushes the overflow global back into the lists and resets its count. (Earlier drafts of this section cited `ToggleRenderpassLists`/`SaveRenderFrameData@0x1401FCAB0`/`0x14020A*` — those names/addresses do **not** exist in this build; they were hallucinated. The real per-pass swap symbol is `CRenderPass::SaveRenderFrameData` @ `0x140194480`.)
- **Drawing does NOT consume:** `CRenderPass::DoDraw` (`0x1401AC7A0`, vtable slot 2) loads `m_CurrentDrawList` (`*(pass+0x30)`), draws `min(m_ListSize, m_NumElements)` blocks via a local cursor, and **never writes `m_NumElements`**. A buffer can be redrawn any number of times.
- **Why eye 1 is empty (running the whole `Draw` twice):** eye 0's rotation points every `m_CurrentDrawList` at the sim-populated buffer → 194 draws. Eye 1's rotation toggles parity *back*, re-points `m_CurrentDrawList` to the buffer eye 0's rotation had just zeroed **and zeroes eye 0's buffer in turn** → 0 draws. The geometry is not consumed by drawing; it is wiped by the *second* rotation.
- **`EraseAllDeletedRenderBlocks` (`0x1401A4ED0`, called `0x1400F23C3`):** a *separate* deletion list; does not touch pass draw lists. Benign across dispatches.
- **Sort task (`m_SortTaskProxy`):** each dispatch creates a fresh sort proxy (`0x1400F1EC3`) that sorts each `m_CurrentDrawList` in place; the dispatch spinwaits before `EndDraw`. Sorting an already-sorted stable list is idempotent → safe across two dispatches (serialized by the single render thread).

**Two correct fixes:**
1. **Gate the rotation on eye 1** — what the mod does (`GATE_ROTATE_RENDER_FRAME_DATA`, default on): detour `0x1401A3000` and skip it on the second dispatch. Eye 1 keeps eye 0's `m_CurrentDrawList` + counts and redraws the identical 194 blocks; also avoids double-toggling the parity and double-flushing the overflow.
2. **Run only the dispatch body twice** (`DispatchDraw`/`HandleDrawThreadTask`) under a single `CGraphicsEngine::Draw` prologue — the rotation then runs once and both dispatches read the same populated buffer. More invasive; the mod instead runs the whole `Draw` twice and gates the per-dispatch side effects.

Either way, **no geometry snapshot/restore is needed** — the draw is non-destructive; you only have to stop the *second rotation* from wiping the lists.

## 12. Per-eye output routing

`Graphics::CopySurfaceToTexture` (`0x14195ABA0`, symbolized `Graphics::ResolveSurface`, called at `0x1400F22B6`) is an **MSAA resolve** (`ID3D11DeviceContext::ResolveSubresource`, or `CopyResource` if non-MSAA) of the **active render-setup color target (final composited scene + HUD) -> the device's presentable BACK_BUFFER surface** (the surface `m_BackBufferLinear` aliases).

**Recommended approach: (b) copy after the resolve, do NOT redirect it.** Let the resolve run normally (keeps the engine's single presentable surface consistent), then `CopyResource` `m_BackBufferLinear`'s texture into your per-eye texture — exactly what the existing mod does in the `PostDraw`-adjacent path. `m_BackBufferLinear` exposes a clean SRV, so it is directly sampleable/copyable. Do the copy on the **immediate context under `m_Context->m_Mutex` (+0x8028)**, ideally right after `0x1400F22B6`, once per eye dispatch. Redirecting the resolve destination per-eye (approach a) fights the device's internal back-buffer-surface state and the alias relationship, and you still need the back buffer valid for the (suppressed) present bookkeeping.

---

## 13. To dispatch one correct stereo eye — checklist

Two viable structures (§11): **(A)** run the whole `CGraphicsEngine::Draw` (`0x1400F4170`) twice and gate/snapshot the per-frame + per-dispatch side effects (what the mod does), or **(B)** run only the **dispatch body** twice under a single prologue. The lists below assume **(A)**; under **(B)** the prologue items (frame counter, jitter phase, CB ring, clock, **list rotation**) run once and need no gating.

**SET (per eye), on the render camera `engine+368` — hook `SetupRenderCamera` (`0x1400B3B80`, `this == engine+0x170`) or right after the active->render memcpy (`0x1400C47E2`). The scene is camera-relative (§2.4), so:**
- `m_TransformF` translation (`+0x84`/`+0x88`/`+0x8C`): lateral IPD/2 offset along the camera right axis (the camera *world position* — this is what camera-relative geometry subtracts). Do NOT just offset `m_View`'s translation; the OffsetVP zeros it.
- Re-derive `m_View = Inverse(m_TransformF)`, then rebuild `m_ViewProjection`/`m_ViewProjectionF` from `m_View x m_Projection` (§2.6) so the per-view CB matches.
- Per-eye off-axis *projection* (optional): write a standard (non-reverse-Z) `m_Projection` (+0x294) before `SetupRenderCamera` so it reverse-Z's + jitters once (§2.7).
- After the dispatch: `WaitForCPUDrawToFinish`, then `CopyResource(eyeN_tex, m_BackBufferLinear)` under `m_Context->m_Mutex` (+0x8028) (§12).

**SNAPSHOT / RESTORE around the eye pair (so once-per-dispatch state doesn't double-step):**
- EffectInfo slot bytes (`engine+4328` + {0,80,160,240,320} `m_FrameIndex`) + index int `engine+4808` (§5.1).
- Per-eye previous-frame VP for velocity if you keep motion blur / TAA mode 3 (§5.7); else disable them.
- (Optional) restore the sort proxy state to skip the eye-1 re-sort (perf only, §11).

**GATE to a single eye:**
- **Render-list rotation `RotateRenderFrameData` (`0x1401A3000`, in the `Draw` prologue) -> skip on eye 1** — else eye 1's rotation flips every `m_CurrentDrawList` to the buffer eye 0 just zeroed and draws no scene geometry (§11). This is the core geometry fix under structure (A); not needed under (B).
- Draw-done `SetEvent`/`engine+20=1` (`0x1400F24A1`) -> **eye 1 (last) only** (§5.6).
- Screenshot countdown `engine+136` -> eye 0 only (§5.5).
- End-draw `sig::Signal` one-shot callbacks (`0x1400F22C2`) -> eye 1 only (§5.5).
- UI block (`0x1400F220E`) -> one eye (or redirect to a UI layer RT, §10).
- Eye-1 render-context `m_Dt = 0` so dt accumulators (fade/exposure/heat-haze) don't double-step (§5.2).

**LEAVE ALONE (do not touch / shared is fine):**
- Draw-list *contents* — non-destructively drawn, so no re-population/snapshot needed (§11). But the per-frame *rotation* must be gated on eye 1 under structure (A) — see GATE above; only leave it alone under (B).
- Shadow parity (`dword_142D3A6B0&1`) — shared slot, benign (§5.3).
- `CClock::Update` — runs once in the sim prologue; do not call per dispatch (§5.4).
- Frame counter / jitter phase / `%3` CB ring — advanced once in the prologue; keep both eyes on the same values (§5.7).
- `Graphics::Flip` — suppressed once per frame via `BLOCK_FLIP`; present via OpenXR.

---

## Open questions / unverified

1. **GBuffer1/2/3 and aux-RT exact formats** beyond GBuffer0 (`ABGR32`): GBuffer1 as `A2R10G10B10` is unverified (out of scope per the brief — best-effort).
2. **Exposure adaptation EMA** exact location/decay (inferred to be in the tonemapping effect; not line-traced). Neutralized by eye-1 `m_Dt=0` regardless.
3. **`unk_142E5B664` / `unk_142E5B670` consumers** (the `%3` ring values): not traced; presumed triple-buffered per-frame resources. Advanced once/frame in the prologue, so benign for double-dispatch.
4. **TAA history RT binding + EffectInfo VP-history struct offset** (§5.7): the camera-side snapshot chain is verified; the downstream history RT names were not traced.
5. **Reflection-proxy RT formats / `VfxDepthCopy_%d` count**: names confirmed, formats not.
6. **Camera-relative per-object combine site (§2.4)**: the mechanism is established from `SetRenderContextCamera` storing a translation-free OffsetVP (`CalculateOffsetViewProjectionMatrix` `0x140136020`) alongside a separate `CameraPosition` (ctx `+0x218` from `m_TransformF+0x84`) — there is no other reason to keep both. The literal per-object `world.row3 -= CameraPosition` multiply in the model render block was not line-verified (a claimed address was a misattribution); not needed for the camera-side fix.
