# Runtime re-initialisation of the render setups (per-eye resolution)

Reverse-engineered from the 2026 Denuvo-less Steam build. This is the RE half of `docs/mod/vr-runtime.md`
"Blocker 2": every render target is sized from the device's display size through
`CGraphicsEngine::CreateRenderSetups`, and this documents who re-runs that function at runtime, what
state it assumes, what it tears down and rebuilds, whether the window swapchain is separable from the
render-target re-init, and the recipe for driving the re-init at a changed size. Data-layout offsets
are byte-stable across builds; only the function addresses below are build-specific (this build).

All addresses are release RVAs from `JustCause3.exe.i64`. The engine calls the graphics-device
backend through the `Graphics::` free functions; the engine class is `CGraphicsEngine`
(the `GraphicsEngine` singleton, `0x142_E2B_6F0`).

## 1. Who calls `CreateRenderSetups`, and when

`CGraphicsEngine::CreateRenderSetups(this, const SDeviceInfo *device_info)` (`0x140_0CE_930`) has
exactly two callers:

1. **`CGraphicsEngine::InitializeSystem`** (`0x140_0F4_560`) — the one-time device + render-setup
   bring-up at startup. Not a runtime path.
2. **`CGraphicsEngine::ApplyResize(this, u32 width, u32 height)`** (`0x140_0CF_A90`) — **the runtime
   resize path**. This is what a windowed resize or a graphics-menu resolution change ultimately
   drives.

### 1.1 How a resize reaches `ApplyResize`

There is no `SetDisplayMode`; the resolution/resize machinery is a small state machine on the engine,
serviced once per frame:

- `CGraphicsEngine::ResizeBuffers(this, width, height)` (`0x140_0D4_3C0`, the engine method, distinct
  from the backend `Graphics::ResizeBuffers`) is the request entry. If
  `m_SynchronousResize && m_HasBeenInitialized` it calls `ApplyResize` inline; otherwise it stashes
  `m_WindowWidth`/`m_WindowHeight` and sets `m_HasNewWindowSettings = 1` to defer.
- `CGraphicsEngine::HandleModeChange(this)` (`0x140_0F4_0C0`) is the per-frame servicing point. When
  `m_DisplayModeChangeState` is idle and `m_HasNewWindowSettings` is set, it calls
  `ApplyResize(m_WindowWidth, m_WindowHeight)`; when a mode change is pending it calls `ApplyMode`
  (`0x140_0F3_AF0`, the fullscreen/adapter path, which recreates the device via `Graphics::ResetDevice`
  rather than resizing buffers).
- **`HandleModeChange` is called from `CGraphicsEngine::Draw` (`0x140_0F4_170`, at `Draw+0x211`)**,
  in the Draw prologue — after the previous frame's render dispatch has been drained
  (`CpuFragmentWaitUntilSignalIsNonZero` on `m_DrawThreadWorkSignal`) and after `Flip`, but **before**
  `Clock::Update`, `HandBackBuffers`, `CalculateConstantBufferIndices`, and `DispatchDraw`. So the
  resize applies on the main/sim thread, at the frame boundary, with no draw in flight and this frame's
  dispatch not yet issued.

`ApplyMode` also sets `m_SynchronousResize = 1` around its `ResetDevice` call so any resize it induces
resolves inline; the resolution menu path funnels through here.

## 2. What `ApplyResize` does, and the state it assumes

Decompiled body (`0x140_0CF_A90`), in order:

1. Clamp `width`/`height` to a minimum of 16.
2. `DestroyRenderSetups(this)` (`0x140_0C4_090`) — tear down every scene render target and render
   setup (see §3).
3. If a UI manager exists, `IUIManager::PrepareForReset` (vtable `+0x38`) — Scaleform drops its
   render-buffer references before the targets go away.
4. **`Graphics::ResizeBuffers(m_Device, width, height)`** (`0x141_952_400`) — **the DXGI swapchain
   resize** (see §4).
5. `Graphics::GetDeviceInfo(m_Device, &device_info)` (`0x141_952_5F0`) — copy the device's now-updated
   `SDeviceInfo` (a plain `memcpy` of `device + 0x190`, `0x38` bytes) onto the stack.
6. **`CreateRenderSetups(this, &device_info)`** — rebuild all render targets at the size in
   `device_info`.
7. Walk `m_RegisteredCallbacksVector` and invoke each registered resize callback with `{width, height}`.
   These are the per-subsystem RT re-allocations (post-effects, SSAO, motion blur, anti-aliasing,
   camera manager aspect, UI, etc.); several of them call `Graphics::GetDeviceInfo` themselves to size
   their own targets.
8. `GetMasterContext` + `SetRenderSetup(ctx, 0, 0)` then `SetRenderSetup(ctx, m_BackBufferRenderSetup, 0)`
   — unbind, then bind the rebuilt back-buffer setup.
9. If a UI manager exists, `IUIManager::RestoreAfterReset` (vtable `+0x40`).
10. `CameraManager.m_AspectRatio = width/height`; `m_Params.m_Width/Height = width/height`;
    `m_HasNewValidDisplayMode = 1`; plus a title-safe area (`4*width/5`, `4*height/5`).

**State assumptions.** The path assumes the immediate context is idle and no render dispatch is in
flight: `DestroyRenderSetups` begins with `SetRenderSetup(masterCtx, 0, 0)` (which unbinds the OM
targets), `Graphics::ResizeBuffers` calls `OMSetRenderTargets(0, …)` and releases the back buffer's
RTV/SRV/texture, and both then free/recreate GPU resources on the device. Because the only caller is
`HandleModeChange` inside the `Draw` prologue, that idle condition is guaranteed by construction: the
previous dispatch was drained and this frame's `DispatchDraw` has not run. No explicit mutex is taken
around `ApplyResize` itself; the resource operations that need the immediate-context critical section
(`Context::m_Mutex`, `device+0x8028`) take it internally through `GetMasterContext`/`SetRenderSetup`.
Running `ApplyResize` off this boundary (e.g. mid-dispatch, or on the render worker) would race the
draw thread against the teardown and is unsafe.

## 3. What `DestroyRenderSetups` tears down (`0x140_0C4_090`)

It first unbinds the active setup (`SetRenderSetup(masterCtx, 0, 0)`), then destroys, in bulk:

- Every engine render setup: `m_GBufferRenderSetup`, `m_BackBufferRenderSetup`,
  `m_PostEffectRenderSetup`, `m_RenderSetupZ`, `m_RenderSetupZAndVelocity`, the debug setups, the AO
  volume setup, the three reflection-proxy setups, and the two downsampled-depth setups.
- Every engine-owned scene texture and surface: MainDepth, MainColor, all four GBuffers, the velocity
  buffer (+ its sRGB alias), DownsampledDepth (+ its alias/second copy), the reflection-proxy
  water-plane/depth/normal-gloss textures, the AO volume, and the five `VfxDepthCopy_%d` slots on
  `m_EffectInfo`.
- The **`BackBufferLinear`** *alias* (`m_BackBufferLinear`).

**What it does not touch:** the DXGI swapchain and its back-buffer surface
(`device->m_BackBufferSurface`, `device+0x8`) are device-owned; only the `BackBufferLinear` alias of
that surface is destroyed here. Everything is nulled after release, so a following `CreateRenderSetups`
re-creates cleanly.

**Stale views / pools to watch (answer to "do stale views linger").** `DestroyRenderSetups` only
frees what the engine holds directly. Render targets owned by the *passes* — the post-effect
fullscreen temp textures and their render setups (`m_FullscreenSrgbTempTexture[3]` /
`m_FullscreenLinearTempTexture[3]` / `m_RenderSetups[3]`), the SSAO pass targets, the SSR pass, the
anti-aliasing history ping-pong, and other per-pass pools — are **not** freed here. They are
re-allocated by the **registered resize callbacks** in step 7. Consequently, if you re-run
`CreateRenderSetups` **without** also running those callbacks, every pass-owned pool keeps its old
dimensions and you get a mismatched pipeline (e.g. per-eye MainColor sampled into an
old-size tonemap temp). Any mod-owned targets (FSR history/output, per-eye capture textures) are
likewise outside the engine's teardown and must be re-created by mod code on a size change. Cross-frame
cached SRVs are otherwise not a hazard at the `Draw`-prologue boundary, because the render context
rebinds all scene SRVs each dispatch.

## 4. Swapchain vs. render-target re-init: are they separable?

**Yes — the render-target sizing is separable from the swapchain, and this is the crux of the blocker.**

`CreateRenderSetups` sizes every *scene* render target from the **`device_info` parameter**
(`device_info->m_DisplayWidth`/`m_DisplayHeight`), not from the device object directly — confirmed in
both the dump and the release decompile (e.g. `refl_tex_params.m_Width = device_info->m_DisplayWidth`
for MainDepth/MainColor/GBuffers/reflection proxies/AO, `>>1` for the half-res depth). So
`CreateRenderSetups(engine, &customInfo)` with a custom `SDeviceInfo` will build all scene RTs at any
size you choose, **without touching the swapchain**.

`Graphics::ResizeBuffers` (`0x141_952_400`) is the part that touches the swapchain, and it is a
distinct call in `ApplyResize` (step 4). It:

- calls `OMSetRenderTargets(0, …)` and releases the back buffer's RTV/SRV/texture,
- calls `IDXGISwapChain::ResizeBuffers(0, width, height, m_BackBufferFormat, 2)`,
- re-acquires buffer 0 and recreates its RTV/SRV,
- writes `device->m_DeviceInfo.m_DisplayWidth/Height/Ratio` (`device+0x1A0/0x1A4/0x1A8`), sets
  `device->m_WasResized` (`device+0x220`).

Skipping this call leaves the game window and its swapchain untouched. **However, the separation is not
total**: at its tail, `CreateRenderSetups` calls `Graphics::GetDeviceSurface(device, BACK_BUFFER)`
(`0x141_956_260`, returns `device->m_BackBufferSurface` = `device+0x8`) and builds **three**
render setups against the *live* swapchain surface, independent of `device_info`:

- `m_BackBufferLinear` = a `SURFACEFORMAT_ABGR32` surface **alias** of the real back buffer,
- `m_PostEffectRenderSetup` = colour → the real back-buffer surface (no depth),
- `m_BackBufferRenderSetup` = colour → `m_BackBufferLinear`, **depth → `m_MainDepthSurface`**, and this
  setup is stored as `m_RenderContext.m_RenderSetup` (the final composite / HUD target).

So if you run `CreateRenderSetups` with a per-eye `device_info` but leave the swapchain at the window
size, the scene RTs (MainColor/MainDepth/GBuffers/…, and the pass pools via the callbacks) become
per-eye sized, while `m_BackBufferLinear` and the two back-buffer setups stay at swapchain size. That
is **not** a benign mismatch: `m_BackBufferRenderSetup` binds `m_BackBufferLinear` (swapchain size) as
colour together with `m_MainDepthSurface` (per-eye size) as depth, and D3D11 requires the RTV and DSV
to share dimensions — the final composite/UI pass would bind a mismatched RTV+DSV pair.

**Consequences for driving per-eye RTs with the swapchain frozen:**

- The scene half is fully separable and viewport-clean (§5).
- The final-composite half (`BackBufferLinear` + the two back-buffer setups) is *entangled with the
  live back buffer through `GetDeviceSurface(BACK_BUFFER)`*. To keep the swapchain at the window size
  **and** have a coherent per-eye final target, mod code must, after `CreateRenderSetups`, replace
  those three setups with a **mod-owned, per-eye-sized "back buffer"** (a texture of your own, its
  render setup rebuilt to pair with the per-eye `m_MainDepthSurface`), rather than the back-buffer
  alias. The mod already suppresses `Graphics::Flip` (`BLOCK_FLIP`) and captures each eye from
  `m_BackBufferLinear` after the resolve (`docs/engine/rendering.md` §7/§12), so the real DXGI back buffer is
  only used as the resolve destination; substituting a per-eye `m_BackBufferLinear` and capturing from
  it keeps the whole scene→composite→capture chain at per-eye resolution with the swapchain never
  resized. The engine's final `CopySurfaceToTexture` resolve into the (still swapchain-sized) back
  buffer must then be skipped or retargeted, since its source would be per-eye sized (the mod's
  existing capture-then-suppress path is the right shape for this).

## 5. Viewports and other size-derived state

**Viewports follow automatically**, confirming `docs/engine/rendering.md` §9. `Graphics::SetRenderSetup`, on
binding a setup, sets the viewport to the bound colour/depth target's own `m_Width`/`m_Height`
(`vp = {0, 0, target->m_Width, target->m_Height, 0, 1}`), so every pass's viewport is its bound RT's
size. Re-running `CreateRenderSetups` at a new size re-creates the RTs at that size and the per-pass
viewports track them with no per-pass viewport patching.

**What is keyed to display size and would NOT follow from `CreateRenderSetups` alone:**

- **Pass-owned RT pools** — re-sized by the registered resize callbacks, not by `CreateRenderSetups`
  (§3). Must run the callbacks (or drive the whole `ApplyResize`).
- **Camera aspect ratio** — `CameraManager.m_AspectRatio` (`CameraManager + 0x5D0`) is set from
  `width/height` in `ApplyResize` step 10, and it feeds the projection built by
  `Camera::RecalcProjection`. For VR the projection is overridden per eye from the HMD FOV
  (`docs/mod/vr-runtime.md` Blocker 1), so the engine aspect matters less, but it is display-size-derived
  and does not follow from `CreateRenderSetups`.
- **TAA jitter scale** — the jitter offset is divided by RT width/height at use, so it tracks the new
  size automatically once the RTs are re-sized.
- **UI / movie-space and world-to-screen mappings** — the HUD/Scaleform view size
  (`CUIManager::m_ViewWidth`/`m_ViewHeight`, used by `Convert3DCoords`/`Get2DInfo`) is refreshed
  through `IUIManager::PrepareForReset`/`RestoreAfterReset` and `CUIManager::UpdateCachedValues`
  (which itself calls `GetDeviceInfo`), i.e. via the resize-callback/UI-reset path, not
  `CreateRenderSetups`. The HUD is composited into `m_BackBufferLinear`, so its target size is whatever
  that surface ends up being (§4) — another reason to give the mod-owned back buffer the per-eye size
  and let the UI reset run against it. Aspect-ratio-derived HUD constants come from the same UI-reset
  path.

## 6. Recommended runtime re-init recipe

Goal: scene render targets (and pass pools) at the HMD per-eye resolution, with the game window and its
DXGI swapchain left at their current size.

**Where (thread + frame position + locks).** Once per real frame, on the **game/sim thread**, at the
top of the mod's per-frame driver in `payload/src/hooks/game.rs` (`game_update_render`), **before the
eye loop's first `game.Draw(spf)`**. This is the same frame position the engine uses for its own
`HandleModeChange` (previous dispatch drained, this frame not yet dispatched), so the idle-context
assumption in §2 holds. Do not run it between eye 0 and eye 1, and not on the render worker. The
internal resource calls take `Context::m_Mutex` themselves; no extra lock is required at this boundary,
but do not overlap it with an in-flight dispatch.

**Sequence** (only when the target per-eye size actually changes):

1. Build a per-eye `SDeviceInfo`: `GetDeviceInfo(engine.m_Device, &info)` to copy the current device
   info, then override `info.m_DisplayWidth`/`m_DisplayHeight` to the per-eye size and recompute
   `info.m_DisplayRatio = width/height`. (Do **not** call `Graphics::ResizeBuffers` — that is the
   swapchain resize.)
2. To make the pass-owned pools and the UI also follow the per-eye size, temporarily write the per-eye
   dimensions into `engine.m_Device->m_DeviceInfo.m_DisplayWidth`/`m_DisplayHeight`/`m_DisplayRatio`
   (`device+0x1A0/0x1A4/0x1A8`) as well, because the registered resize callbacks and
   `CUIManager::UpdateCachedValues` size their targets from `GetDeviceInfo` (i.e. from
   `device->m_DeviceInfo`), not from the callback's `{width,height}` argument. This does **not** touch
   the swapchain — only the cached device-info dimensions. Keep the real values if any code elsewhere
   depends on window size, and restore them if needed.
3. Drive the re-init. Two options:
   - **Reuse the engine path (simplest, correct for the scene + pools + UI):** call
     `engine.ApplyResize(per_eye_width, per_eye_height)` **with the `Graphics::ResizeBuffers` call
     neutralised** (hook/patch that one call to a no-op for the duration), so the swapchain is left
     alone but `DestroyRenderSetups` → `CreateRenderSetups` → all resize callbacks → UI reset run
     normally. This gives per-eye scene RTs, per-eye pass pools, and a consistent UI reset in one call.
   - **Hand-roll (more control, more surface):** `DestroyRenderSetups(engine)`;
     `IUIManager::PrepareForReset`; `CreateRenderSetups(engine, &info)`; run the registered resize
     callbacks; `IUIManager::RestoreAfterReset`. This mirrors `ApplyResize` minus the swapchain resize
     but requires the mod to walk the callback vector itself.
4. Fix up the swapchain-tied final targets (§4): after `CreateRenderSetups`, replace `m_BackBufferLinear`
   and rebuild `m_PostEffectRenderSetup` / `m_BackBufferRenderSetup` against a mod-owned per-eye
   texture paired with the per-eye `m_MainDepthSurface`, so the final composite/HUD/capture chain is
   per-eye and the real back buffer is untouched. Capture each eye from that per-eye surface and skip
   (or retarget) the engine's `CopySurfaceToTexture` resolve into the real back buffer.
5. Re-create any mod-owned size-dependent resources (FSR history/output, per-eye capture textures) to
   the per-eye size in the same pass.

## 7. Risks and open unknowns

- **Step 4 is the load-bearing unknown for "swapchain frozen".** Whether replacing the three
  back-buffer-derived setups is cleaner than simply resizing the swapchain buffers to the per-eye size
  is a design call. Note that `Graphics::ResizeBuffers` resizes the *swapchain buffers*, not the Win32
  window (it never calls `SetWindowPos`), and the mod already suppresses `Flip`; so resizing the
  swapchain to the per-eye size would give a fully coherent per-eye `BackBufferLinear` for free, with
  the only visible cost being a (suppressed) stretch on any present. If keeping the DXGI buffers at the
  window size is not a hard requirement, driving the full `ApplyResize` (swapchain included) is the
  lowest-risk path; the frozen-swapchain approach trades that simplicity for the step-4 target
  substitution. This is untested either way and should be validated at runtime.
- **Frequency / hitching.** `DestroyRenderSetups` + `CreateRenderSetups` free and re-allocate a large
  set of GPU resources; doing it per frame would hitch badly. Trigger it only on an actual per-eye size
  change (once at session start, and on runtime reconfiguration), not every frame.
- **Registered-callback set is not enumerated here.** The recipe relies on the resize callbacks
  re-sizing the pass pools; the exact membership of `m_RegisteredCallbacksVector` was not enumerated.
  Reusing `ApplyResize` (option 3a) sidesteps this by letting the engine walk the vector.
- **`ApplyMode` / fullscreen path** (`0x140_0F3_AF0`) recreates the device via `Graphics::ResetDevice`
  rather than resizing buffers; it is not needed for the render-scale use case and was not fully
  traced.
- **`m_DisplayModeChangeState` value semantics** are only partially confirmed (idle vs. mode-change
  pending); it is recorded as a `u32` with the observed behaviour rather than a fully enumerated enum.
- The exact offsets of `m_MainDepthSurface` and `m_PostEffectRenderSetup` on the engine were not pinned
  (only `m_BackBufferRenderSetup` at `+0x1050` and `m_BackBufferLinear` at `+0x1230` are); the step-4
  fix-up will need those pinned before implementation.
