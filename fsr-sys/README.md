# fsr-sys

FFI bindings to AMD FidelityFX Super Resolution 2 (FSR2) with the native DirectX 11 backend, used by the payload for VR anti-aliasing (and, later, upscaling). See `docs/fsr.md` for the integration design.

The C++ backend is the vendored `optiscaler/FidelityFX-FSR2-DX11` submodule (MIT) at `vendor/FidelityFX-FSR2-DX11`. `build.rs` compiles it for the MSVC target alongside the shader headers under `generated/`.

## Shaders

The DX11 backend bakes its compute shaders in as DXBC bytecode: AMD's shader compiler (`FidelityFX_SC.exe`, bundled in the submodule) turns each pass into a `<pass>_permutations.h` header of byte arrays that the backend `#include`s. Those generated headers live under `generated/dx11/` and are **git-ignored for now** (124 files of bytecode, too noisy to commit while the integration is in flux), so you must generate them once after checkout before the crate will build. `build.rs` fails with this instruction if they're missing:

```sh
cargo run -p fsr-shadergen --target x86_64-unknown-linux-gnu
```

The regenerator is the separate `fsr-shadergen` crate — kept out of `fsr-sys` so its HTTP/archive dependencies never reach the `-sys` crate or the payload. It is a host-side tool (hence the explicit host target — the workspace default target is Windows), self-provisioning and reproducible from a clean checkout:

- `FidelityFX_SC.exe` is a Windows executable. On a Windows host it runs directly; elsewhere it runs under Wine.
- Under Wine, `FidelityFX_SC.exe`'s `-compiler=fxc` path needs a **native** `d3dcompiler_47.dll` — Wine's built-in reimplementation rejects FSR's shaders. The tool downloads one in-process (from the Firefox redist, the standard winetricks source) and installs it into a managed Wine prefix under `target/fsr-shader-build/`. Both the DLL and the prefix are cached, so only the first run hits the network.

Requirements (non-Windows host): `wine` on `PATH` (provided by `shell.nix`) and network access on the first run. The download and 7-zip extraction are done in-crate (`ureq` + `sevenz-rust2`), so no `curl`/`7z` needed.

Environment overrides (all optional): `FFX_SC`, `FSR_VENDOR_DIR`, `FSR_GENERATED_DIR`, `WINE`, `WINEPREFIX`, `D3DCOMPILER_DLL` — see the module docs in `fsr-shadergen/src/main.rs`.

The compile recipe (pass list, the `FFX_HALF` 16-bit variants, and the compiler args) mirrors the upstream CMake (`src/ffx-fsr2-api/CMakeLists.txt` + `src/ffx-fsr2-api/dx11/CMakeLists.txt`); if a version bump changes those, update `PASSES` / the `*_ARGS` constants in `fsr-shadergen` to match.
