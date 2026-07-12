# JC3 per-frame sun-shadow and global-lighting pipeline

Reverse-engineered from the 2025 Denuvo-less Steam build of Just Cause 3 (Apex engine). This doc maps how the game computes and stages its per-frame sun-shadow and global-lighting state — the constants the material shaders read out of `cb0` GlobalConstants each frame, where each value comes from, and, critically, **which of them are selected by a frame counter or frame parity** (the set of things that legitimately differ frame-to-frame in the base game). Addresses are release RVAs; struct offsets are byte-stable across the Denuvo/Denuvoless/debug builds (only `.text` moved). Names follow the pyxis-defs; follow a name into the defs for the exact offset.

Companion to [`rendering.md`](rendering.md) (the frame pipeline and the stereo double-Draw machinery) and [`shaders.md`](shaders.md) (the shader bytecode side). Where `rendering §N` is cited, it refers to that doc.

## 1. The frame counters

Three engine-global counters and one per-context stamp select frame-varying state. All live in [`render_frame_counters`](graphics_engine::graphics_engine::RenderFrameCounters) (`0x142D3A6AC`), advanced **once per real frame** in the `GraphicsEngine::Draw` prologue (`Draw+0x66`, rendering §1.3):

| Counter | Address | Advance | Selects |
|---|---|---|---|
| `m_Counter` | `0x142D3A6AC` | `m_FrameIndex = m_Counter++` — post-increment each frame | The **rolling counter** for shadow-cascade amortization (§3.1) and the light triple-buffer (§6). Read as the full value, not just parity. |
| `m_FrameIndex` | `0x142D3A6B0` | takes the pre-increment `m_Counter` | The **frame parity** `m_FrameIndex & 1`: shadow atlas double-buffer (§3), TAA jitter phase (rendering §5.7), shadow-matrix storage stride. |
| `m_RingIndex` | `0x142D3A6B4` | `m_FrameIndex % 3` | The three-slot constant-buffer / light-lookup ring. |
| `RenderContext.m_RenderFrameNo` | ctx `+0x448` | copied from `m_FrameIndex` in `SetupRenderContext` (`+0xC8`) | The per-dispatch stamp the draw side keys shadow parity on. **Equals `m_FrameIndex`**, so `m_RenderFrameNo & 1 == m_FrameIndex & 1`. |

**Parity consistency (the key invariant).** The shadow *simulation* writes its parity storage indexed by `m_Counter & 1` (`UpdateRender+0x29`), while the *draw* side reads by `m_FrameIndex & 1` / `m_RenderFrameNo & 1`. These agree because the sim shadow update runs in `CGameStateRun::UpdateShadows` (sim phase, **before** the `Draw` prologue increments the counter): at sim time `m_Counter == C`, and the prologue then assigns `m_FrameIndex = C`. So sim-write parity `C & 1` == draw-read parity `m_FrameIndex & 1`. Both dispatches of a stereo frame share one prologue, hence one parity — the second eye reads the same shadow half the first eye did (rendering §5.3).

`SetupRenderContext` (`0x140174060`) also stamps the context CB-ring indices (`m_FrameIndex % 3` at ctx `+0x44C`, `(m_FrameIndex+2) % 3` at ctx `+0x450`) and copies the live per-frame light/sun scalars into the context (point/spot light counts, the light-manager sun direction, the intensity scales, and `GetTimeOfDayLightModulator`).

## 2. Cast of state owners

- **`CLightManager`** (singleton `qword_142ED0E70`): the scene light gather. Holds the sun direction and the global intensity scales; gathers the frame's visible point/spot lights into a parity/triple-buffered update list ([`CopyLightsToUpdate`](graphics_engine::light_manager::LightManager::CopyLightsToUpdate)).
- **`CShadowManager`** (reached via `CGraphicsEngine::GetShadowManager`): the cascaded sun-shadow system (§3). Enable flag mirrored in `m_SystemEnabled` (`unk_142ED75BB`).
- **`CRenderEngine`** (singleton `qword_142ED0E18`): owns the GlobalConstants staging block and the time-of-day sun/fog scalars; `SetGlobalShaderConstants` (§5) assembles the CB here.
- **`RenderContext`** (per dispatch, `engine+1824`): the per-view scratch the passes fill and read.

## 3. Sun-shadow simulation (sim phase)

The sun shadows are an 8-slot cascaded atlas (`m_AtlasTexture`, a depth `Texture2DArray`), parity double-buffered: [`m_SliceBase[2]`](graphics_engine::shadow_manager::ShadowManager::m_SliceBase) (`ShadowManager+0x61F0`) gives the base array slice for each frame parity, so alternate frames render into (and the shaders sample) opposite halves of the array.

### 3.1 `SetActiveShadowPassCount` (`0x14018A7D0`) — the amortization schedule

Called from `CGameStateRun::UpdateShadows` before the fit. It sets the active cascade+spot-shadow count and rebuilds the per-cascade update schedule. Each cascade `c` carries an update *level* in [`m_CascadeUpdateLevels`](graphics_engine::shadow_manager::ShadowManager::m_CascadeUpdateLevels) (default pattern `{0,1,2,3}` — nearest every frame, each further one half as often). A cascade **refreshes this frame** only when `((1 << level) - 1) & m_Counter == 0`; otherwise it copies its previous fit forward. This is the mechanism that amortizes cascade re-renders across frames, and it is gated on the **full rolling `m_Counter`** (`0x142D3A6AC`), not on parity — so which cascades are freshly fitted cycles with period `2^level`, independently of the parity double-buffer.

### 3.2 `UpdateRender` (`0x1401C7370`) — the per-frame fit

Runs once per sim frame. `v6 = m_Counter & 1` is the parity it writes. For the active camera (`CameraManager.m_ActiveCamera`, whose `m_ProjectionF` gives the fit frustum) it, per scheduled cascade:

- computes the cascade's world frustum (`CFrustum::Compute`) and calls `UpdateCascade` to fit the slice;
- writes the **parity-indexed** cascade box params at `this + 32*(9*parity + 405)`, the per-cascade blend/scale at `this + 288*parity + 12976`, the cascade reference vectors at dword `3*parity + 2316`/`+2322`, and the **per-slice shadow projection matrices** at `this + 512*parity + 64*slice + 14928` (0x40 each);
- unscheduled cascades (the amortized ones, §3.1) fall through the `else` branch and keep the prior parity's fit;
- finally `GenerateCullPlanes` regenerates the cull planes.

So the shadow manager holds two full parity copies of every cascade's transform and box params; the fresh half is the one just written, the stale half is the previous frame's — and amortized cascades leave even the "fresh" half carrying an older fit.

### 3.3 `CommitRenderPassSettings` (`0x1401779C0`) — the per-dispatch gate

Runs per dispatch (`HandleDrawThreadTask+0xB8`, gated by `unk_142ED75BB`). `v3 = m_FrameIndex & 1`. It clears [`m_Enabled`](graphics_engine::render_pass::RenderPassState) on every static/dynamic shadow pass, then re-enables the passes the schedule marked active this frame and re-points each enabled pass's render target at the parity's atlas slice (`this + 1120*cascade + …`, reading the `20*parity`-strided per-cascade pass indices). This is where the parity chosen at sim time becomes the atlas half actually rendered and sampled.

### 3.4 `GetShadowFade` (`0x140177940`) / `m_ShadowTransparency`

`GetShadowFade` returns `m_ShadowTransparency` (`ShadowManager+0x325C`) when shadows are enabled, else `1.0`. **This is a static config value, not a per-frame dynamic term:** its only writer is `CShadowManager::SetShadowTransparency` (a one-line setter; dump `SetShadowTransparency` — release address not yet pinned), and it is registered as the debug console variable `"Dev|Graphics|Shadows|Transparency"` (default `0.0` from the constructor). It is **stable** frame-to-frame unless the console var is changed. `SetGlobalShaderConstants` stages it as `1 - fade` and `fade` (§5). It does **not** track sun visibility.

### 3.5 The sun-grazing shadow swing

There is no dynamic sun-occlusion query feeding the shadow strength. The whole-terrain shadow swing seen when the sun grazes the horizon comes from two ground-truth mechanisms, both continuous rather than a discrete flag:

1. **The minimum sun-elevation clamp** in `SetGlobalShaderConstants` (§5.1): the staged lighting sun direction is rotated up whenever its elevation drops below a floor angle, so near the horizon a small change in the true sun direction produces a comparatively large change in the clamped direction the shaders light with.
2. **Cascade re-fit** (§3.2): rotating the camera re-fits the scheduled cascades to the new frustum; when the sun is near-grazing, the fit's basis is ill-conditioned (light direction nearly in the fit plane), so the per-slice shadow matrices swing as the frustum turns. The amortized cascades (§3.1) update on their own schedule, so different cascades swing on different frames.

## 4. Deferred lighting combine

`CRenderBlockDeferredLighting::DrawClustered` (`0x14013CFD0`, `RP_DEFERRED_LIGHTS` = `0x5C`) does the clustered light assignment and the deferred combine into MainColor. It reads the per-view constants out of the **render context** `a2` (light counts at ctx `+0x3E8`/`+0x3F8`, camera position at ctx `+0x288`, projection scalars at ctx `+0x2FC`/`+0x310`/`+0x314`), binds the global CB (`SetFragmentProgramConstantBuffer 0 = a2[101]`), and calls `CLightManager::SetupLightingTextures`. It consumes the GlobalConstants and the shadow atlas the previous stages staged; it does not itself select frame-indexed state. The sun contribution and cascade sampling ride the GlobalConstants block (§5).

## 5. `SetGlobalShaderConstants` (`0x140185740`) — the centerpiece

Called per dispatch (`HandleDrawThreadTask+0x166`) after `SetRenderContextCamera`. It assembles the `cb0` GlobalConstants block into a staging region on `CRenderEngine` (dword fields from ~`this[1600]` on; the block is later bound as the global vertex/fragment CB). It runs only when the light manager singleton is present (`qword_142ED0E70`), first calling `CLightManager::ApplyDynamicLights` + `UpdateRenderContext`. What it stages, grouped:

### 5.1 Sun / light direction (with the elevation clamp)

- **Raw sun direction** `(v7,v8,v9)` = light-manager sun direction → staged at `this[1815..1817]`.
- **Elevation-clamped sun direction** `(v95,v96,v97)`: starts as the raw direction, then `v15 = dot(sunDir, worldUp)` with `worldUp = (0,0,1)` (`xmmword_142D39890`). If `v15 < sinf(minElevation)` where `minElevation = this[1409]` (a `CRenderEngine` float), it builds `RotationAxis(cross(sunDir, worldUp), -(minElevation - asin(v15)))` and rotates the direction up to the floor. Staged at `this[1927..1929]`. This is the term behind the near-horizon lighting swing (§3.5).
- **Sun basis / colour products** from `CRenderEngine` fields `this[1404..1414]` (time-of-day sun colour × basis, set by the environment/lighting update) → `this[1919..1938]`. Continuous, same both dispatches.

### 5.2 Shadow terms (the parity surface)

- **Shadow atlas slice base** `v20 = m_SliceBase[m_RenderFrameNo & 1]` (`ShadowManager + 4*parity + 0x61F0`) → staged at `this[1626]`, `this[1630]`, `this[1922]`. **Parity-indexed.**
- **Secondary slice** `v21 = *(ShadowManager+0x61F8)` when `*(bool)(ShadowManager+0x61FC)` else `0` → `this[1630]`.
- **Shadow fade** `v47 = m_ShadowTransparency` (or `1.0` when disabled, §3.4) → `1 - v47` at `this[1839]`, `v47` at `this[1840]`. Stable.
- **Cascade block** `memcpy(this+7932, ctx+0x654, 0x120)` — the forward-material [`m_ShadowCascades`](graphics_engine::graphics_engine::RenderContext::m_ShadowCascades) (per-cascade scale/blend + offset/slice + box-test params). Filled by `SetRenderContextCamera` from the shadow manager's **parity** storage. **Parity-indexed.**
- **Shadow matrices** `memcpy(this+8220, ctx+0x454, 0x200)` — the 8 per-slice projective [`m_ShadowMatrices`](graphics_engine::graphics_engine::RenderContext::m_ShadowMatrices), likewise parity-filled. **Parity-indexed.**

### 5.3 Camera-derived constants

- **Full (translation-bearing) view-projection** `memcpy(this+6428, RenderCamera->m_ViewProjectionF, 0x40)` (render cam `+0x194`) — drives screen-space / non-geometry work (rendering §2.4). Off-axis, so **per-eye** in stereo.
- **Camera world position** from `RenderCamera->m_TransformF` translation row (render cam dwords `+29..31` and `+33..35`, i.e. `m_TransformF` rows 2/3) → `this[1623..1630]`, `this[1819..1830]`. **Per-eye.**
- **Depth-unproject constants** from the render camera near/far: `1/m_Near` (cam `+0x594`) and `1/m_Far` (cam `+0x598`) → `this[1841] = 1/near − 1/far`, `this[1842] = 1/far`. Stable within a frame.
- **Inverse RT size** from `GetDeviceInfo`: `1/width`, `1/height` → `this[1837]`, `this[1838]`.

### 5.4 Wetness and fog

- **Wet properties** `memcpy(this+7852, GetWetProperties(ctx), 0x40)` — the weather wetness block. Continuous.
- **Fog / atmosphere** via `SetFog` (`0x140184F40`) at the tail: builds two fog layers from `CRenderEngine` fog scalars — fog colour `this[1334..1342]` → `this[1699..1706]` and `this[1911..1918]`; per-layer distance/density ramp (start/inv-range/height) into `this[1663..1670]` and `this[1859..1866]`; and a normalized fog light direction into `this[1979..1982]`. Fog reads the context view distance (`ctx+0x28C`) and the time-of-day fog fields; continuous, same both dispatches.

## 6. Light gather buffering

`CLightManager::CopyLightsToUpdate` (`0x1400C6860`) copies the frame's visible point/spot lights into `m_FrameLightInfo[3]` / the `m_LightLookup[3]` / `m_LightIndexBuffer[3]` triple-buffer, indexed off `m_Counter`, and configures the volumetric spot-light passes ([`enable_low_res_spot_light_volume`](graphics_engine::light_manager::enable_low_res_spot_light_volume)). The dynamic light list the deferred combine (§4) reads is therefore selected by the rolling counter, one of the three slots per frame.

## 7. The ping-pong table

Every per-frame global lighting/shadow value the material shaders consume, and what selects it frame-to-frame. "Stable" = deterministic within a frame and identical across both dispatches of a stereo frame; "per-eye" = legitimately differs between the two dispatches by design; "parity" = `m_FrameIndex & 1` (== `m_RenderFrameNo & 1`); "counter" = full `m_Counter`.

| Value | Where computed | Backing field / global | Indexed by |
|---|---|---|---|
| Raw sun direction | `SetGlobalShaderConstants+0x160` | light mgr sun dir → CB `this[1815..1817]` | stable |
| Elevation-clamped sun direction | `SetGlobalShaderConstants+0x1DF` (RotationAxis clamp) | CB `this[1927..1929]`; floor angle `CRenderEngine this[1409]`; up = `0x142D39890` | stable (continuous fn of sun dir) |
| Sun colour / basis products | `SetGlobalShaderConstants` | `CRenderEngine this[1404..1414]` → CB `this[1919..1938]` | stable (time-of-day) |
| Time-of-day light modulator | `SetupRenderContext+0x209` | `GetTimeOfDayLightModulator` → ctx `+0x398` | stable |
| Intensity scales (env/vfx/volumetric) | `SetupRenderContext` | light mgr `+0x0C..0x14` → ctx `+0x408..0x410` | stable |
| **Shadow atlas slice base** | `SetGlobalShaderConstants+0x2B3` | `m_SliceBase[parity]` (`ShadowManager+0x61F0`) → CB `this[1626/1630/1922]` | **parity** |
| **Shadow cascade block** (scale/blend, offset/slice, box test) | filled by `SetRenderContextCamera`, staged `SetGlobalShaderConstants+0x1BC0` | ctx `m_ShadowCascades` (`+0x654`) → CB `this+7932` | **parity** |
| **Per-slice shadow matrices [8]** | `SetRenderContextCamera`, staged `SetGlobalShaderConstants` | ctx `m_ShadowMatrices` (`+0x454`) → CB `this+8220` | **parity** |
| Active cascade count | `SetRenderContextCamera` | shadow mgr parity byte → ctx `+0x774` | **parity** |
| **Which cascades are freshly fit** (atlas contents) | `SetActiveShadowPassCount` schedule + `UpdateRender` | `m_CascadeUpdateLevels`; gate `((1<<lvl)-1)&counter` | **counter** (period `2^lvl`) |
| Shadow fade / transparency | `SetGlobalShaderConstants+0x6F5` | `m_ShadowTransparency` (`ShadowManager+0x325C`) | stable (console var) |
| Shadow enable | throughout | `m_SystemEnabled` (`unk_142ED75BB`) | stable |
| Full view-projection (`m_ViewProjectionF`) | `SetGlobalShaderConstants+0x757` | render cam `+0x194` → CB `this+6428` | per-eye |
| Camera world position | `SetGlobalShaderConstants` | render cam `m_TransformF` translation → CB `this[1623..1630/1819..1830]` | per-eye |
| Depth-unproject (1/near, 1/far) | `SetGlobalShaderConstants+0x45F` | render cam `m_Near`/`m_Far` (`+0x594`/`+0x598`) | stable within frame |
| Inverse RT size | `SetGlobalShaderConstants+0x67F` | `GetDeviceInfo` | stable |
| Wet properties block | `SetGlobalShaderConstants+0x538` | `GetWetProperties(ctx)` → CB `this+7852` | stable |
| Fog colour / ramps / fog light dir | `SetFog` | `CRenderEngine this[1334..1365]` → CB `this[1663..1982]` | stable (time-of-day) |
| Dynamic light list | `CopyLightsToUpdate` | `m_FrameLightInfo[3]`, `m_LightLookup[3]` | **counter** (3-slot ring) |
| TAA jitter phase (perturbs projection) | `ApplySubsampleJitter` | reads `m_FrameIndex` | **parity/ring** (rendering §5.7) |
| Auto-exposure | `CToneMappingEffect::Update` | `m_CurrentExposure` | frame-counted (rendering §5.4) |

**The shadow ping-pong surface, in one line:** the shadow slice base, the cascade block, the per-slice matrices, and the active-cascade count all switch on `m_FrameIndex & 1`; *which* cascades carry a fresh fit switches on the full `m_Counter`. Everything else in the lighting/fog/sun set is recomputed every frame from continuous inputs and is identical across a stereo frame's two dispatches. Any lighting/shadow flicker that is *not* explained by the exposure meter (rendering §5.4) or TAA jitter must come from one of the parity- or counter-indexed shadow rows above being read at a parity/counter that disagrees with the half that was written.

## 8. New field / function findings (for pyxis promotion)

Established here and not yet in the defs; capture as engine-neutral definitions:

- `ShadowManager::m_ShadowTransparency: f32` at `0x325C` — the global sun-shadow transparency (0 = opaque, 1 = no shadow), registered as the console variable `"Dev|Graphics|Shadows|Transparency"`; returned by `GetShadowFade`. Writer `ShadowManager::SetShadowTransparency` (release address not yet pinned).
- `ShadowManager` secondary slice at `0x61F8` with a validity `bool` at `0x61FC`, read by `SetGlobalShaderConstants` alongside `m_SliceBase`.
- `RenderEngine::SetFog(&mut self, ctx, apply_far: bool)` at `0x140184F40` — builds the two-layer fog GlobalConstants from the engine's time-of-day fog scalars (`CRenderEngine this[1334..1365]`).
- `RenderEngine` time-of-day lighting scalars: sun colour/basis at dword fields `1404..1414`, minimum sun-elevation floor angle at `1409`, fog colour at `1334..1342`, fog distance/height ramp at `1353..1365`.
- `RenderBlockDeferredLighting::DrawClustered(&mut self, ctx, a3, tex)` at `0x14013CFD0` — the `RP_DEFERRED_LIGHTS` clustered light-assignment + deferred combine.
- `LightManager` scalar fields observed in release: `m_TimeOfDayLightModulator` at `+0x00`, intensity scales (`m_VFXLightIntensityScaleFactor`, `m_VolumetricLightIntensityScale`, `m_CutSceneLightIntensityScaleFactor`) at `+0x0C..0x14`, and a sun-direction vector read at `+0x24..0x2C`. The `+0x24` offset conflicts with the 2016 dump's `CLightManager` layout (where `m_SpotLightInstances[192]` begins there); the release light-manager layout should be re-derived before this vector is promoted.
</content>
</invoke>
