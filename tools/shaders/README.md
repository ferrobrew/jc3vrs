# Shader reverse-engineering tools

Helpers for pulling apart Just Cause 3's shaders. The full walkthrough — bundle format, how to read
the disassembly, how to find a specific effect, and how the mod patches shaders — is in
[`docs/engine/shaders.md`](../../docs/engine/shaders.md). Quick reference:

```sh
# 1. Carve the DXBC blobs out of a bundle (one of the four *.shader_bundle in the game dir).
python3 extract_dxbc.py "$HOME/.steam/steam/steamapps/common/Just Cause 3/Shaders_F.shader_bundle"
#    -> ./Shaders_F.shaders/sh_0000_xxxxxxxx.dxbc ...

# 2. Disassemble one to SM5 assembly.
./disasm.sh Shaders_F.shaders/sh_0467_0016b270.dxbc | less
```

Prerequisites for `disasm.sh` (all already used elsewhere in this repo):

- the xwin sysroot at `.xwin/xwin` — run `scripts/xwin_build.sh` once if absent;
- the `d3dcompiler_47.dll` + wine prefix under `target/fsr-shader-build/` — run
  `cargo run -p shadergen --target x86_64-unknown-linux-gnu` once to provision them;
- `wine` and an unwrapped `clang` on `PATH`.

`disasm.exe`, the copied `d3dcompiler_47.dll`, and any extracted `*.shaders/` dirs are gitignored
(rebuilt on demand).
