# Static foveated rendering (issue #29)

The mod renders the scene twice per frame (once per eye) at full shading rate everywhere, including
the periphery where the lenses and human vision barely resolve detail. Static (fixed-centre) foveation
reduces the peripheral shading cost. This is the design; the survey of alternatives (compositor FFR,
NVAPI VRS, quad views) lives in issue #29 — the short version is that the only **cross-vendor, D3D11**
path is Valve-style **radial density masking** via the stencil buffer, so that is what this documents.

## The idea

Before the scene render, write a radial mask into the main depth-stencil's **stencil** bits: peripheral
pixels get a mask bit set. Then, for the expensive scene passes, enable a stencil **test** that discards
the masked (peripheral) pixels before the pixel shader runs. After the scene render, a cheap fill-in
pass reconstructs the discarded pixels from their neighbours. Works on any GPU and any API that has a
stencil buffer — JC3's main depth surface is `D32FS8`, so the 8-bit stencil is there.

## Why it is feasible in JC3 (the RE)

Reverse-engineered against the 2026 Denuvo-less Steam build (imagebase `0x140000000`).

### One central depth-stencil seam

JC3 does **not** build a fresh `ID3D11DepthStencilState` per pass or scatter `OMSetDepthStencilState`
across render blocks. It uses a deferred, dirty-tracked, cached model:

- Render blocks mutate a **bit-packed 64-bit DS-state index** and a stencil ref on the context through
  granular setters (`Graphics::SetStencilTestEnable` `0x141966730`, `SetStencilFunc` `0x141966680`,
  `SetStencilOp` `0x1419666F0`, `SetStencilMask` `≈0x1419666B0`, `SetDepthFunc`, …). These only write
  context fields — no D3D call.
- The actual bind happens in exactly one place: **`Graphics::SetupRenderStates` `0x14195FEA0`**, the
  pre-draw state flush at the top of every `Draw*` wrapper. It reads the packed index, looks it up (or
  lazily creates it, `Graphics::CreateDepthStencilState` `0x14195F9D0`) in a per-context
  `map<u64, ID3D11DepthStencilState*>`, and issues the **sole** `OMSetDepthStencilState` (device-context
  vtable +0x120 / index 36) with the current stencil ref. It re-binds only when the index or ref changed.

Context (`HContext_t`) fields (to be defined in pyxis, not accessed by raw offset):

| field | offset | note |
|---|---|---|
| `ID3D11DeviceContext*` | `+0x8020` | the wrapped immediate context |
| current DS-state packed index (u64) | `+0x8CC0` | staged by the setters, flushed by `SetupRenderStates` |
| current stencil ref (u32) | `+0x8CCC` | bound alongside the DS state |

Packed DS index bit layout (from `CreateDepthStencilState`): low dword — bit0 `DepthEnable`, bit1
`DepthWriteMask`, bits2–5 `DepthFunc`, **bit6 `StencilEnable`**, bits7–10 `StencilFunc`, bits11–18
`StencilReadMask`, bits19–26 `StencilWriteMask`, bits27–30 `StencilFailOp`; high dword — bits0–3
`StencilDepthFailOp`, bits4–7 `StencilPassOp`.

So a hook on `SetupRenderStates` can OR a stencil **test** into the packed index just-in-time, and the
lazy cache self-manages the extra state object. This is the force-test seam.

### The engine already uses stencil — the override must be selective

Stencil test/write is baked into the shipped per-pass `RenderSettings` data (`[15]` StencilTestEnable,
`[11]` func, `[12]` read mask, `[13]` write mask, `[14]` ref, `[8..10]` ops) and staged into
`rc->StencilSettings`. Observed engine usage on the main surface:

- **bits 5+6 (`0x60`)** — SMAA edge mask.
- **bit 6 (`0x40`)** — character skin / SSS.
- **bit 0 (`0x01`)** — outline-effect blur.
- data-driven decal/road masks (`RP_ROAD_STENCIL` `53`), plus `RP_CLEAR_STENCIL` `141`.

**Bit 7 (`0x80`)** is not referenced by any engine *code* path found, so it is the mask-bit candidate —
but the shipped `RenderSettings[13]` write masks could not be audited offline (no Gibbed tooling in the
repo). Therefore the mask bit is a **config value** (default `0x80`); a collision shows as corruption in
one effect and is a one-line change. The override must **skip passes that already enabled stencil**
(checkable via `rc->StencilSettings.StencilTestEnable`, staged from `RenderSettings[15]`) — D3D11 has one
stencil test per draw, so we cannot stack a second test on SSS/outline/SMAA/road; those are cheap and
render unmasked.

### Clear timing and the mask-write injection

Depth-stencil clears are per-pass and data-driven through `Graphics::Clear` `0x141967020` (flags bit0 =
depth, bit1 = stencil). The main depth+stencil is cleared early in the GBuffer range around
`RP_Z_OCCLUDERS` (`47`). **The mask must be written after that clear.** The existing `DrawRenderPassRange`
hook (`payload/src/hooks/graphics_engine/render_pass.rs`) already splits pass ranges, so it hosts the
insertion: after pass `47`, draw a full-screen radial-mask pass (bind MainDepth DSV; DS state =
`StencilEnable`, func `ALWAYS`, `StencilOp REPLACE`, write mask = the mask bit, ref = the mask bit,
`DepthEnable=0`/`DepthWriteMask=0`, no colour target; the pixel shader discards foveal-centre pixels so
only the periphery writes the bit), then continue the range.

## Implementation

The design above is implemented, on by default, across:

- **pyxis defs** — `Graphics::SetupRenderStates` (`0x14195FEA0`), `Graphics::CreateDepthStencilState`
  (`0x14195F9D0`), and the `HContext_t` `m_DepthStencilStateIndex` (`+0x8CC0`) / `m_StencilRef`
  (`+0x8CCC`) fields with the full packed-index bit layout.
- **shaders** — `payload/src/shaders/foveation_mask_ps.hlsl` (radial dithered mask, interleaved-gradient
  noise) and `foveation_fill_ps.hlsl` (neighbour-average reconstruction), compiled via `shadergen`. Both
  take a 32-byte cbuffer `{ centre_px, inner_px, outer_px, max_drop }`; the two must keep their `ign` and
  drop decision identical so the fill reproduces the mask's choice.
- **`payload/src/vr/foveation.rs`** — the D3D pipeline (mask-write and fill-in full-screen passes on the
  immediate context under `Context::m_Mutex`, the `blit.rs` pattern), the `SetupRenderStates` force-test
  index rewrite (`apply_force_test`), and the `packed_stencil_test` bit math.
- **`payload/src/hooks/graphics_engine/render_pass.rs`** — the `SetupRenderStates` detour, and the
  `DrawRenderPassRange` bracketing that runs the mask-write before the foveated pass range, forces the
  stencil test through it, and runs the fill-in after. The foveal centre comes from the eye's
  `projection_standard` principal point (`foveal_center_uv`), so canted/off-axis eyes fovea about their
  true optical centre, not screen centre.
- **config + UI** — `foveation.{enabled, inner_fraction, outer_fraction, max_drop, mask_bit,
  foveal_first_pass, foveal_last_pass}` (`config.rs`), surfaced in the "Foveation" debug section.
  `enabled` defaults **true**.

## Open items (needs in-headset validation)

The feature ships on by default. It is untested on hardware and touches the hottest render path, so it
can be disabled (config or debug UI) if it perturbs the concurrent #30 / #31 validation. What
still needs confirming in a headset:

- **The default foveated pass range** (`0x41 RP_MODELS_DYNAMIC` … `0x4B RP_CREATURES`) is a starting
  guess. It must sit *after* the depth prepass (so dropped pixels keep full-resolution depth) and cover
  the expensive shading without breaking a pass that reads neighbour pixels. Tune `foveal_first_pass` /
  `foveal_last_pass` against what actually saves frametime and what corrupts.
- **`DrawRenderPassRange` call structure.** The bracketing assumes the scene range arrives as one call (or
  a few sequential sub-ranges); the mask/fill are gated to run once each even if split. If the engine
  interleaves the range differently, the injection points need revisiting.
- **Engine depth-stencil state cache.** The force-test relies on `SetupRenderStates` re-flushing because
  the rewritten index differs from the last-flushed one. Confirm the first foveated draw actually rebinds
  (so the mask-write's transient DS state does not leak into a scene pass), and that the first post-range
  draw restores the normal state.
- **Back-face stencil.** The packed index is assumed to drive both faces; if `CreateDepthStencilState`
  leaves the back face `ALWAYS`, back-facing peripheral geometry would still shade. Verify against a scene
  with visible back faces.
- **Stencil-bit audit.** Confirm no shipped pass writes the mask bit (`0x80`); mitigated by the bit being
  configurable.
- **Quality tuning** — the density ramp, radii, and fill neighbourhood need validation under lens
  distortion; not reproducible offline.

## Future: dynamic (eye-tracked) foveation

If the runtime exposes `XR_EXT_eye_gaze_interaction`, the mask centre recentres on gaze each frame — the
same masking machinery, only the `foveal_centre_uv` cbuffer moves. Out of scope until the static path is
proven.
