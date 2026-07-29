# Shader reverse-engineering tools

Helpers for pulling apart Just Cause 3's shaders. The full walkthrough — bundle format, how to read
the disassembly, how to find a specific effect, and how the mod patches shaders — is in
[`docs/engine/shaders.md`](../../docs/engine/shaders.md). Quick reference:

```sh
# 1. Carve the DXBC blobs out of a bundle (one of the four *.shader_bundle in the game dir).
python3 extract_dxbc.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
#    -> ./Shaders_F.shaders/sh_0000_xxxxxxxx.dxbc ...

# 2. Name them, from the bundle's own ADF name table (index/offset match the carved filenames).
python3 shader_names.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
#    -> 0000 00004810 vertex   2dtex1 ...   (--json for machine-readable, --scan for the fallback)

# 3. Disassemble one to SM5 assembly.
../../scripts/dxbc.sh disasm Shaders_F.shaders/sh_0467_0016b270.dxbc | less

# 4. Or compile an HLSL reference shader to DXBC (e.g. to learn what fxc emits for a transform).
../../scripts/dxbc.sh compile ref.hlsl main vs_5_0 > ref.dxbc
```

`scripts/dxbc.sh` wraps the `dxbc-tool` crate (`tools/dxbc-tool`), which calls `D3DCompile` /
`D3DDisassemble` through the `windows` crate. It cross-builds the tool with cargo-xwin and runs it
under wine. Prerequisites (all already used elsewhere in this repo):

- the xwin sysroot at `.xwin/xwin` — run `scripts/xwin_build.sh` once if absent;
- the shared wine prefix and `d3dcompiler_47.dll`, provisioned by
  `scripts/wine_prefix.sh` — source it and call `jc3vrs_ensure_wine_prefix` (run
  `cargo run -p shadergen --target x86_64-unknown-linux-gnu` once to trigger provisioning);
- `wine` on `PATH`.

Any extracted `*.shaders/` dirs are gitignored (rebuilt on demand).
