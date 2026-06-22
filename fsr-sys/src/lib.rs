//! FFI bindings to FidelityFX Super Resolution 2 (FSR2) with the native DirectX 11 backend.
//!
//! Wraps the vendored `optiscaler/FidelityFX-FSR2-DX11` submodule (MIT). The C++ backend and the
//! committed shader-permutation headers (`generated/`) are compiled by `build.rs`; this module
//! exposes the resulting `ffx_fsr2` / `ffx_fsr2_dx11` C API to the payload.
//!
//! Bindings are not yet wired up -- this is the crate skeleton.
