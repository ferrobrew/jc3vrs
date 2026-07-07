# JC3 shaders: extraction, disassembly, and interpretation

How to get at Just Cause 3's compiled shaders, turn them back into readable assembly, find the one you
care about, and patch it from the mod. The tooling referenced here lives in
[`tools/shaders/`](../../tools/shaders/). Reverse-engineered against the 2026 Denuvo-less Steam build;
struct offsets and bundle layout are byte-stable across builds, only function addresses move.

## Where the shaders live

The game ships four shader bundles in its install root (next to `JustCause3.exe`):

| Bundle | Selected when |
|---|---|
| `Shaders_F.shader_bundle` | normal shadow quality |
| `ShadersLowShadows_F.shader_bundle` | low shadow quality |
| `ShadersConstMath_F.shader_bundle` | Intel GPU (`VendorId 0x8086`), normal shadows |
| `ShadersConstMathLowShadows_F.shader_bundle` | Intel GPU, low shadows |

Each is an **ADF container** (Avalanche Data Format — magic `' FDA'` / `41 44 46 20` little-endian)
that packs one **DXBC** blob per shader permutation (~1550 in `Shaders_F`). `CSettingsManager::UpdateSettings`
picks the bundle name (`"Shaders"`, `"ShadersLowShadows"`, `"ShadersConstMath"`,
`"ShadersConstMathLowShadows"`; the `_F.shader_bundle` suffix is added internally) by shadow quality and
GPU vendor, then calls `CGraphicsEngine::LoadShaderBundle`.

## Extracting the DXBC

`extract_dxbc.py` scans for the `DXBC` container magic and uses each container's embedded total-size
field (a u32 at `+0x18`), so it doesn't need to parse the ADF structure:

```sh
python3 tools/shaders/extract_dxbc.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
# -> Shaders_F.shaders/sh_0000_0003dac0.dxbc ...   (sh_<index>_<byte-offset-in-bundle>.dxbc)
```

Repacking the bundle is **not** needed (and not implemented): the mod patches shaders in memory at
load (see [the patch seam](#how-the-engine-creates-shaders-the-patch-seam)), so the on-disk bundles are
left untouched.

## Disassembling a blob

`disasm.sh` builds a tiny `D3DDisassemble` harness (`disasm.c`) with clang + the repo's xwin sysroot
and runs it under wine against the `d3dcompiler_47.dll` that `shadergen` provisions:

```sh
./tools/shaders/disasm.sh Shaders_F.shaders/sh_0467_0016b270.dxbc | less
```

See [`tools/shaders/README.md`](../../tools/shaders/README.md) for the one-time prerequisites. The output
is standard FXC-style SM5 assembly: a commented reflection header (cbuffers, resource bindings, the
input/output signatures) followed by the instruction stream.

Caveat: the harness writes **CRLF** line endings (it runs under wine), so `grep`/`awk` on the saved
output want `grep -a` or a `sed 's/\r$//'` first — `disasm.sh` already strips them on stdout.

## Reading the disassembly

The header is the fast way in. For example, a sun-lit opaque material shader declares:

```
// Resource Bindings:
// DiffuseMap / NormalMap / PropertiesMap      texture     t0..t2     // material
// ShadowMapTexture            texture  2darray   t14                 // the cascaded shadow map
// ShadowComparisonFilter      sampler_c          s15                 // PCF compare sampler
// CloudShadowMap / HorizonMap0 / HorizonMap1                         // other occlusion terms
// GlobalConstants / LightingFrameConsts        cbuffer  cb0/cb3
//
// Input signature:
// SV_Position  0  POS  ...   TEXCOORD 0..4 ...                       // world pos / TBN carried in TEXCOORDs
```

Then in the body, `dcl_*` declares the registers (`v0` = `SV_Position`, etc.), and the work follows.
Useful reflexes:

- **Identify the shader from its resources.** A fullscreen pass has `SV_Position`-only input and binds
  GBuffers + depth; a forward material shader has `TEXCOORD` inputs and material textures. Shadow
  resolves bind `ShadowMapTexture` (`t14`) + `ShadowComparisonFilter` (`s15`) and emit
  `sample_c_lz_indexable ... t14 ... s15` taps.
- **`v0.xy` is the screen pixel.** Any math seeded off `v0.xy` is screen-space, which in stereo means it
  differs between the eyes (the same world point lands on a different pixel per eye).
- **Match the assembly to the engine pass** via the resource names: e.g. SSAO binds
  `DeinterleavedDepthTexture`; motion blur binds `VelocityTexture`/`NeighborMaxTexture`.

## Finding a specific shader

Resource and cbuffer names live in the DXBC `RDEF` chunk as ASCII, so you can grep the raw blobs
without disassembling everything:

```sh
# which blobs sample the cascaded shadow map?
grep -la ShadowMapTexture Shaders_F.shaders/*.dxbc | wc -l        # ~167

# distinct occlusion/AO/noise resource names across the bundle
cat Shaders_F.shaders/*.dxbc | strings -n6 | grep -iE 'shadow|ssao|ambient|noise|jitter' | sort | uniq -c
```

Then disassemble the candidates and confirm by their instructions. The engine-side names (what each
pass/effect is called, what reads what) are in [`rendering.md`](rendering.md); follow a resource name
there to the pass that binds it.

## How the engine creates shaders (the patch seam)

`Graphics::CreateFragmentProgram(device, params)` is the only caller of
`ID3D11Device::CreatePixelShader`. `params` (`CreateFragmentProgramParams`, laid out in pyxis-defs) is
just the DXBC bytecode and its length. The returned `HFragmentProgram_t` holds only the
`ID3D11PixelShader*`; **no DXBC is retained**. `CreatePixelShader` copies the bytecode, so a hook can
substitute a patched copy that only has to outlive the call.

The mod uses this to patch shaders in memory (`payload/src/hooks/graphics_engine/shader.rs`):

1. **Hook `CreateFragmentProgram`** — scan `params.m_Code` for the target byte pattern; if present,
   point `m_Code` at a patched copy for the duration of the call, then restore it. After editing the
   bytecode, **recompute the DXBC container checksum** (a modified MD5 over everything past the 20-byte
   header, stored at offset `0x4`) and write it back — the D3D stack under Proton validates it and
   rejects a blob whose stored hash no longer matches, so a patch that skips this step silently fails to
   create the shader. See `refresh_dxbc_checksum` / `dxbc_hash` in `shader.rs`.
2. **Force a reload** — because injection happens after the game has built its shaders, the hook only
   sees them when they are re-created. `CGraphicsEngine::LoadShaderBundle(name)` reloads a bundle (and
   re-creates every shader holder through `CreateFragmentProgram`), but only if `name` differs from the
   current (`m_CurrentBundleName`, laid out in pyxis-defs). The mod's "Reload shaders"
   button bounces the active bundle to its other quality variant and back, which re-creates everything
   through the hook. Changing shadow quality in the game's own graphics menu does the same.

This means a shader fix is a **byte pattern + a patch**, applied live and reversibly, with the on-disk
bundles never modified.

## Case study: the per-eye shadow PCF rotation hash (issue #10)

A worked example of the whole loop, and the reasoning behind the shader patch the mod ships.

**Symptom.** Sun shadows (and alpha-tested foliage) shimmer/grain differently between the two eyes in
stereo.

**Finding.** The opaque sun-shadow resolve (the ~159 material shaders that bind `ShadowMapTexture`)
rotates its 38-tap Poisson PCF disk by a hash of the screen pixel:

```
dp2     r#.w, v0.xyxx, l(12.989800, 78.233002, 0, 0)   ; v0 = SV_Position (screen pixel)
sincos  r#.w, null, r#.w
mul     r#.w, r#.w, l(43758.546875)
frc     r#.w, r#.w                                       ; fract(sin(dot(pixel, k)) * 43758.5)
sincos  rot_sin, rot_cos, r#.w                           ; -> per-pixel PCF disk rotation
```

The shadow *lookup* itself uses the interpolated world position, identical per eye, so the shadow's
position and mean coverage match — only the per-pixel tap **noise** differs, because the same world
point maps to a different pixel (hence a different rotation) in each eye. The constant `12.9898` occurs
**only** in this instruction (159 sites per bundle), so it is an exact fingerprint.

**Fix.** Zero the two seed constants (`12.9898`, `78.233`) in that `dp2`, i.e. patch the 16-byte
immediate `39 d6 4f 41 4c 77 9c 42 00 00 00 00 00 00 00 00` to leading zeros. The dot product becomes
0, the rotation a constant, and both eyes sample the same unrotated 38-tap disk — with 38 taps the look
change is negligible. The mod does this in the `CreateFragmentProgram` hook above
(`stereo.patch_shadow_pcf_hash`), applied via the "Reload shaders" button.

> A raw byte-patch invalidates the DXBC container hash; `D3DDisassemble` rejects the blob
> (`hr=0x80004005`), and — contrary to an earlier assumption that only the disassembler cares — the D3D
> stack under Proton rejects it too: the patched shadow shaders fail to create and the scene renders
> near-black with speckle. The mod therefore recomputes the checksum after patching
> (`refresh_dxbc_checksum`), which makes the blob valid for every consumer; recomputing it on a blob you
> patch on disk likewise makes `D3DDisassemble` accept it again.

**What this does *not* explain.** A rotated disk changes the noise, not the mean, so it does not make a
shadow uniformly stronger or longer in one eye. That residual is **inherent view-dependence** in
screen-space AO (each eye computes SSAO fresh from its own depth, so near an occluder one eye sees more
of it), not an accumulation bug — the sun-shadow map and its cascades are computed once per frame from
the shared camera and are identical between eyes, and the one history-buffered darkening pass (SSAO
temporal) is already gated per eye. A genuinely "shared" AO would need to reproject one eye's result to
the other. See `payload/src/hooks/graphics_engine/{shader,ssao}.rs` and `payload/src/debug/rt_hash.rs`
(the per-eye RT-hash diagnostic that confirms there is no cross-eye accumulator).
