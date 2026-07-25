# The mod-owned back buffer

While a VR session runs, the engine renders into a texture the mod allocates rather than into the DXGI swapchain, so the render resolution and the swapchain size stop being the same number. The scene is per-eye sized; the swapchain stays at the window's client size; the desktop mirror's present is 1:1.

Controlled by `vr.own_back_buffer` (default on). Implemented in `payload/src/vr/back_buffer.rs`, with two hooks in `payload/src/hooks/graphics_engine/graphics_engine.rs` and the transitions in `payload/src/vr/resolution.rs`.

Addresses are release RVAs from the 2026 Denuvo-less Steam build. The engine-side behaviour this rests on is documented in [`render-setups-reinit.md`](../engine/render-setups-reinit.md) and [`rendering.md`](../engine/rendering.md); this document is the mod-side design.

## 1. Why

The engine builds its final composite target, `m_BackBufferLinear`, as a **format alias of DXGI back buffer 0** — `Graphics::CreateSurfaceAlias` copies the source handle wholesale, takes a D3D11 reference on the same resource, and builds fresh views over it. The composite pass therefore writes directly into the presentable surface, and there is no copy afterwards.

The consequence for VR is that driving the scene to the per-eye render resolution drags the swapchain along with it, because the only way to resize `m_BackBufferLinear` is to resize the thing it aliases. That costs three things:

- The desktop present rescales every frame from a near-square per-eye buffer onto a 16:9 window, so the mirror needs an aspect pre-compensation and the egui overlay is round-tripped through a stretch.
- Single-pass double-wide forces a 2x-width swapchain, for a buffer only ever half-shown.
- "Back buffer size" and "render size" are the same number, so mod code that wants one reads the other and is right by accident.

Substituting a mod-owned texture for the three swapchain-derived objects breaks the link. The scene chain, the composite, and the per-eye capture all stay at render resolution; the swapchain is left to be what it is, the thing the window presents.

## 2. The substitution set

`Graphics::GetDeviceSurface(device, surface)` (`0x141_956_260`) is a two-line accessor returning `device + 8` for the `BACK_BUFFER` selector and null otherwise. It has **exactly one code cross-reference in the binary**, inside `CGraphicsEngine::CreateRenderSetups` — so the set of engine objects derived from the swapchain surface is closed, and it is three fields plus one mirror of a field.

The tail of `CreateRenderSetups` (`0x140_0CE_930`) builds them:

| Store | Engine offset | Built from |
|---|---|---|
| `m_BackBufferLinear` | `+0x1230` | `CreateSurfaceAlias` over `GetDeviceSurface(BACK_BUFFER)` at format `28` (`ABGR32`), name `"BackBufferLinear"` |
| `m_PostEffectRenderSetup` | `+0x1238` | `CreateRenderSetup` with colour[0] = the raw device back-buffer surface, no depth |
| `m_BackBufferRenderSetup` | `+0x1050` | `CreateRenderSetup` with colour[0] = `m_BackBufferLinear`, depth = `m_MainDepthSurface` (`+0x1060`) |
| `m_RenderContext.m_RenderSetup` | `+0xEA0` | the same pointer as `+0x1050` |

Their readers, established by grepping the symbol dump and confirming each against the release decompile:

- **`m_BackBufferLinear`** — `CreateRenderSetups` (as the colour target of `m_BackBufferRenderSetup`); `DestroyRenderSetups`, which frees it with `Graphics::DestroySurface` and nulls it; `HandleDrawThreadTask` and `HandleScreenShot`, which pass it to `CGraphicsEngine::SaveScreen` on the screenshot paths; `CUIManager::InitPlatformRT`, which takes its RTV alongside `m_MainDepthSurface`'s DSV — the one the HUD redirect already displaces. `CGraphicsEngine::GetBackBufferLinear` is an accessor with no callers in either build.
- **`m_PostEffectRenderSetup`** — read only in `HandleDrawThreadTask`, three times in succession: `SetRenderSetup`, then as the first argument of `CPostEffectsManager::ApplyWorldFilters` and of `CRenderEngine::DrawPosteffects`.
- **`m_BackBufferRenderSetup`** — read in `HandleDrawThreadTask` (the final composite and UI target), in `ApplyResize` step 8, and in `ApplyMode`.
- **`m_RenderContext.m_RenderSetup`** — the render context's copy, consumed wherever the context's setup is read back.

Replacing the three objects and writing the mirrored copy therefore covers every consumer. Nothing else in the engine reaches the swapchain surface except `Graphics::ResizeBuffers` and `Graphics::Flip`, which own it legitimately.

## 3. The two hooks

**`CreateRenderSetups` epilogue — the substitution.** `CreateRenderSetups` has exactly two callers, `InitializeSystem` and `ApplyResize`, so every path that rebuilds the engine's originals funnels through it and an epilogue hook cannot be missed. On return, with the engine's three objects freshly built, the mod ensures its backing texture exists at the device-info size, builds a surface and two render setups over it, installs them, and frees the engine's originals.

The size comes from `device->m_DeviceInfo`, which is what `CreateRenderSetups` has just built everything else from — so the substitute's dimensions agree with `m_MainDepthSurface`'s, and the composite setup can pair the two. That agreement is the reason to substitute here rather than anywhere earlier.

The hook deliberately ignores `CreateRenderSetups`' return value. Its declared C++ type is `bool` and the symbol-bearing dump ends in `return 1`, but **release codegen dropped that return**: the last instruction before the epilogue is the trailing `CreateRenderSetup` call, so `al` carries the low byte of the pointer it returned — reliably even, so a bit-0 test on it is always false. Gating the substitution on it silently disabled the whole feature.

**`Graphics::ResizeBuffers` (`0x141_952_400`) — a substitute, not a suppression.** `ApplyResize` calls this between `DestroyRenderSetups` and `CreateRenderSetups`, then immediately reads the new size back out of `device->m_DeviceInfo` and feeds it to `CreateRenderSetups` and to every registered resize callback. A plain no-op would leave the whole pipeline at the old size.

So the detour skips the DXGI half — the `OMSetRenderTargets(0, …)`, the view and texture release, the `IDXGISwapChain::ResizeBuffers`, the buffer-0 re-acquire — but still writes `m_DeviceInfo.m_DisplayWidth`, `m_DisplayHeight`, and `m_DisplayRatio`, sets `m_WasResized`, and returns success. `ApplyResize` then runs verbatim: scene targets, pass pools, UI reset, and the camera aspect all follow the render size, while the DXGI buffers never move.

`device->m_BackBuffer`'s own `m_Width`/`m_Height` are written only by the real function, so under the substitute they keep reporting the true swapchain size. That is the point: after this, `m_BackBuffer` means "the window" and `m_BackBufferLinear` means "the render target", and the two stop being conflated (§6).

Both hooks are gated on the same ownership flag, so they cannot land one without the other — a half-installed pair would give `m_BackBufferRenderSetup` a window-sized colour against a per-eye depth, which D3D11 does not define behaviour for. The gate also keeps `Graphics::ResizeBuffers` doing its real job on the device-reset paths and on eject.

## 4. Ownership

`Graphics::DestroySurface` (`0x141_953_9C0`) has **no already-freed guard on this build**: no active/inactive bookkeeping survives release codegen, so a double destroy is an unguarded use-after-free that corrupts silently and surfaces somewhere unrelated, later. There is one ownership rule, and it carries the whole design:

- The three substitute objects live in the **engine's own fields**, and the engine's `DestroyRenderSetups` frees them correctly, because they are engine-allocated objects sitting where engine-allocated objects belong. The mod never holds a second pointer to any of them, and never frees them itself.
- The **backing texture** is the one object the engine knows nothing about — nothing in `DestroyRenderSetups` corresponds to it. The mod owns that handle alone and frees it exactly once, with `Graphics::Destroy2DTexture`, after the engine has been told to stop using it.

Two orderings follow, and both are load-bearing:

- Within the substitution, install the new pointers *before* destroying the old ones, so every field stays non-null for the whole window. `CreateRenderSetups` only runs on the drained idle context, so no other thread is looking, but the ordering costs nothing.
- Across a transition, clear the ownership flag *before* the resize that rebuilds the engine's originals, never after. Clearing it afterwards leaves the engine bound to a texture about to be freed.

`DestroyRenderSetup` unbinds a setup from the device if it is the active one, so destroying a bound setup is safe. Nothing registers surfaces by name on this build — the resource tracking that read `params.m_Name` is compiled out — so the substitute's name (`"VrBackBuffer"`) is for whoever reads a memory dump, and can collide with nothing.

## 5. Lifecycle

**Entering.** `resolution.rs` sets the ownership flag, then requests the per-eye size through the engine's own deferred resize path. The `ResizeBuffers` substitute holds the swapchain; the `CreateRenderSetups` epilogue performs the substitution, in the same `ApplyResize`.

An ownership change forces a resize even at an unchanged size, because `ApplyResize` is what carries the substitution in either direction — without that, toggling `vr.own_back_buffer` mid-session would appear to do nothing until the next resolution change.

**Bringing the swapchain down.** Taking ownership stops the swapchain *following* the render size, but it does not move a swapchain already oversized from an earlier resize — and mid-session, that is exactly the state. So `back_buffer::sync_swapchain_to_window`, called from the frame top, compares the swapchain against the window client rect and drives a real `Graphics::ResizeBuffers` when they differ. It is a no-op unless ownership is live, the substitution is installed, and the sizes disagree, so it doubles as window-resize following.

This is only possible *because* of the substitution: the engine's alias on buffer 0 is gone, and `IDXGISwapChain::ResizeBuffers` fails while any reference to buffer 0 outstands. The mirror's cached views are the mod's own such references, so they are dropped first and rebuilt lazily on the next frame.

The call is bracketed by a bypass flag, so the `ResizeBuffers` substitute stands aside for it, and by a save and restore of `m_DeviceInfo`'s display dimensions — the real function writes those, and here that write is exactly wrong. The device info must keep meaning "the render size", which is what the substitution decoupled it from the swapchain to say; left clobbered, the render-size driver sees the size collapse to the window's, requests the per-eye size again, and every sync sets off another resize-and-resubstitute round trip.

**Leaving, and eject.** Clear the flag first, then resize to the window's live client size. Both hooks are now inert, so this is the stock path: `DestroyRenderSetups` frees the substitutes (engine-allocated, so correctly), the real `Graphics::ResizeBuffers` runs, and `CreateRenderSetups` rebuilds the engine's own alias and setups over the live swapchain. Only then is the backing texture destroyed.

On eject there is no second chance, because the game thread tears down without drawing another frame — so `resolution.rs::on_shutdown` runs that sequence synchronously rather than through the deferred path, after `WaitForCPUDrawToFinish`. The synchronous restore is split into its own function so that its several early exits cannot skip the texture release that must follow it. The detour uninstalls happen later, in `shutdown_startup`, which is safe as long as the flag is already clear: a stray `CreateRenderSetups` in between does nothing.

The restore targets the window's **live** client size, falling back to the size captured at take-over. The engine follows WM window resizes itself, so a window moved during the session makes the capture stale, and restoring it would leave the game rendering at the wrong size for its own window.

## 6. Where the frame goes, and what reads its size

Nothing is suppressed, so the flow after substitution is the engine's own, one target over:

1. `HandleDrawThreadTask` runs post-effects into `m_PostEffectRenderSetup` (mod texture, no depth), then the composite and Scaleform UI into `m_BackBufferRenderSetup` (mod texture plus the per-eye `m_MainDepthSurface`), then `QuickDrawController::Draw`, then `Graphics::EndDraw`. All at render resolution.
2. The mod's `RenderEngine::PostDraw` hook captures from `m_BackBufferLinear` into the per-eye capture textures — the same code as before, now reading the substitute.
3. `BLOCK_FLIP` suppresses `Graphics::Flip`, so the engine never presents.
4. `vr/blit.rs` blits the captures into the OpenXR swapchain slices and submits.
5. `vr/mirror.rs` draws the chosen eye into `device.m_BackBuffer` — now genuinely window-sized — composites the egui panel or the flat overlay, and presents.

The mirror is the composite, and it already existed. This adds no present path; it makes the existing one 1:1.

Because `m_BackBuffer` and `m_BackBufferLinear` now answer different questions, every consumer has to pick one. `crate::stereo::render_size` reads `m_BackBufferLinear` and is the render basis; `per_eye_render_size` halves it under single-pass double-wide. Anything sizing a resource *to the render* uses those — the per-eye capture textures, the F10 capture, the HUD's reset proxy, and its engine-binding restore. Only code addressing the presented surface reads the swapchain's size: the mirror's viewport, and the cursor mapping's window basis.

The HUD texture is neither. It is a flat panel at a fixed apparent size, so what it needs is enough texels to look crisp at that size — a property of the headset and the panel's angular size, not of the scene's pixel count. `hud.render_resolution` pins it outright; otherwise it derives from `render_scale * sqrt(eye_width * eye_height)`. The geometric mean, rather than the longer axis, keeps the budget stable as the render target's shape changes: a per-eye target is much taller than it is wide, and the older `render_scale * max(w, h)` inflated the HUD 2x under double-wide.

## 7. Interactions

**`vr/resolution.rs`** keeps its trigger unchanged — it still writes `m_WindowWidth`, `m_WindowHeight`, and `m_HasNewWindowSettings`, and lets `HandleModeChange` service the request from the `Draw` prologue, the one frame position where the idle-context assumption holds by construction. Its completion detection still works, because the `ResizeBuffers` substitute writes the same device-info fields it polls, and its window-rect cross-check becomes *more* meaningful, since now nothing in the chain has any business touching the window. What it gained is the ownership state machine (§5).

**`vr/mirror.rs`**'s buffer→window pre-compensation self-neutralises: with the swapchain equal to the client rect, its `sx` and `sy` are both 1 and the buffer-space viewport is the window-space one. It is still live whenever `vr.own_back_buffer` is off, so it stays; a unit test pins the equal-size case so it can be deleted outright once ownership stops being a flag. `hud::mirror_overlay` is window-sized and stretched across the whole buffer to invert the present's stretch, which with buffer == window is at last pixel-exact rather than round-tripped.

**The HUD redirect** (`hud/binding.rs`) is unaffected: it replaces Scaleform's RTV and DSV with mod-owned views regardless of what `m_BackBufferLinear` points at, and `hud::tick` re-applies it every frame from the render thread.

**The flat egui overlay** renders into `device.m_BackBuffer`'s RTV from the `graphics_flip` detour, but only when `BLOCK_FLIP` is clear, i.e. only outside a session — where the substitution is inactive (§8). It is the one mod path that draws into the swapchain outside the mirror.

## 8. Flatscreen

The substitution is inactive whenever the mod is not driving a render size of its own: `resolution.rs` takes ownership only while an XR session is running with `vr.native_resolution` on, and only with `vr.own_back_buffer` set.

It buys nothing outside that. In flatscreen the engine's render size *is* the window size, so no divergence exists, and substituting a texture of identical dimensions would add an indirection, a second allocation, and an eject-path liability in exchange for nothing — while breaking the flat overlay's assumption that the engine presents (§7). The framing *once the mod injects, the game never renders directly to the swapchain again* is the steady state during VR, not an invariant from `DllMain`; making it literal would mean owning the swapchain through the launcher, the loading screens, and any flatscreen play.

Flatscreen stereo sits between the two: it renders two eyes but presents to the same window at window resolution. If it ever wants a render size different from the window it becomes a session-like case and the same predicate covers it; until then it follows the flatscreen rule.

## 9. The engine surface API

All four constructors and both destructors are ordinary `Graphics::` free functions taking the device, and all six are used by `CreateRenderSetups`/`DestroyRenderSetups` — so calling them from mod code is doing what the engine does. They are defined in `graphics_engine/surface.pyxis`, along with their parameter structs and enums.

A "surface" in this engine is an `HTexture_t` obtained either from `GetRenderTarget` over a texture or from `CreateSurfaceAlias` over another surface; there is no `Graphics::CreateSurface`. `CreateRenderSetups` builds every scene target as `Create2DTexture` → `GetRenderTarget`, and the substitute follows that pattern.

A render setup reads no width or height at all — its dimensions come entirely from its bound targets, which is why `SetRenderSetup` can synthesize a full-target viewport from them. The substitute setups use the values `CreateRenderSetups` uses for both back-buffer setups: `m_MultisampleFormat = 1`, `m_Mask = 15`, and the `+0xD0` bitfield at `0x79` (`m_AutoResolve = 1`, `m_EDRAMLayout = 0`, `m_UAVStart = 15`, the "immediately after the colour targets" sentinel).

Both parameter structs are written through a raw pointer into zeroed storage rather than materialised as Rust values. The engine leaves several fields at zero, and zero is not a valid discriminant for every enum among them — `TileMode` has none — so producing either struct by value would be instant undefined behaviour. Writing field-by-field through the pointer makes the bytes exactly what the engine's own call sites pass, without ever asserting enum validity.

### 9.1 One texture, no alias

The substitute is a single `ABGR32` texture filling both the `m_PostEffectRenderSetup` colour-target role and the `m_BackBufferLinear` role. No alias and no `m_Castable` is needed, because there is no format gap to bridge.

`CGraphicsEngine::InitializeSystem` hardcodes `m_FrontBufferFormat = 28` and leaves the adjacent `m_FrontBufferColorSpace` zero, both in one 8-byte write. `Graphics::InitializeDevice` applies `MakeSRGBFormat` only when that colour-space flag is set, so the format passes through unconverted to `device->m_BackBufferFormat` and to `sd.BufferDesc.Format` for `CreateSwapChain`: **the swapchain is `R8G8B8A8_UNORM`, not the `_SRGB` variant.** The same function writes that value into the back-buffer `HTexture_t`'s format field, so `GetDeviceSurface(BACK_BUFFER)->m_Format` is `28` too.

`ESurfaceFormat` defines `SURFACEFORMAT_ABGR32 = 0x1C` = 28, with no separate sRGB enumerator — sRGB-ness is orthogonal and applied at creation time. For these common formats the engine's enum *is* the DXGI numbering, so 28 means `DXGI_FORMAT_R8G8B8A8_UNORM` in both namespaces, and `m_BackBufferLinear`'s requested format equals its source's native format exactly.

This also puts evidence under the mirror's gamma reasoning, which argues from the swapchain being non-sRGB that no conversion belongs in the mirror path.

### 9.2 What `CreateSurfaceAlias` actually does

It memcpys the source `HTexture_t`, takes a D3D11 reference on the resource, then:

```
if ( source->m_Texture != device_backbuffer->m_Texture || requested_format == device_backbuffer->m_Format )
    build a fresh SRV at requested_format
else
    no SRV
build a fresh RTV at requested_format   /* unconditionally */
```

The SRV is skipped only when aliasing the device back buffer's own resource **at a format that differs** from its native one — presumably because that resource is not typeless, so a foreign-format SRV would be invalid. When the formats match, as they do here, the normal branch runs and a genuine SRV is built. So `m_BackBufferLinear` is an ordinary alias with a real reference and real view objects; it is cheap because there is no format gap, not because the engine special-cases it into a no-op.

That reference is also why `ApplyResize` destroys the setups before calling `Graphics::ResizeBuffers`, and why an independent swapchain resize is possible at all once the alias is gone (§5).

## 10. Not covered

- The engine's screenshot paths (`SaveScreen` from `HandleDrawThreadTask`, `HandleScreenShot`) now capture at render resolution rather than window resolution. They read the surface rather than the swapchain, so they should be fine; this is unverified, and the mod has its own capture path anyway.
- Whether Scaleform's `RestoreAfterReset` behaves with a mod-owned `m_BackBufferLinear`. The HUD redirect re-applies immediately afterwards regardless, so the intervening frame is unobserved.
- Whether freezing the DXGI buffers changes anything about DXVK's present path — filtering, latency, or frame pacing — beyond removing the downscale.
- Promoting `hud::binding::movie_viewport_matches` from a supplementary drift check to the HUD's primary reset signal. The reset proxy is keyed on the render size, which is correct for a render-size change, but the viewport comparison is the signal it stands in for and would detect a reset regardless of what size anything is.

## 11. A note on symbols

The release IDB's `Graphics::` names are demonstrably unreliable. This work found two mislabelled functions whose names are swapped with each other: `0x141_95A_BA0`, labelled `Graphics::CopySurfaceToTexture`, is really `Graphics::EndDraw(ctx)`, and `0x141_954_850`, labelled `NGraphicsEngine::CGPUProfiler::BeginScope`, is really `Graphics::CopySurfaceToTexture(ctx, dst, src)`. `CGraphicsEngine::SaveScreen` (`0x140_0DE_C90`) is labelled `CGPUProfiler::EndFrame`, and `Graphics::CreateRenderSetup` (`0x141_954_5F0`) was already known mislabelled as `LockVolumeTexture`.

That mislabelling had a real cost: it is what made an earlier investigation conclude the engine performed a final resolve into the back buffer that would have to be suppressed or retargeted — the hardest-looking part of the plan, and a thing that does not exist. Every address recorded here was verified by call-site correspondence or body inspection rather than by name, and any further symbol taken from that namespace must be too.
