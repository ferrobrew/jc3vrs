//! Compiles the vendored FSR2 DX11 backend and links it into the crate.
//!
//! The shader-permutation headers under `generated/dx11/` are produced by the `fsr-shadergen` crate
//! (and git-ignored for now), so a fresh checkout must run it once before this builds. We check for
//! them up front and fail with that instruction rather than emitting a wall of missing-include errors
//! from the C++ compiler.

use std::path::{Path, PathBuf};

/// One representative generated header; its absence means the whole set needs regenerating.
const SENTINEL_HEADER: &str = "generated/dx11/ffx_fsr2_rcas_pass_permutations.h";

/// The FSR2 backend translation units we compile (the portable core + the DX11 backend + its baked
/// shader blobs). Paths are relative to the vendored submodule root.
const SOURCES: &[&str] = &[
    "src/ffx-fsr2-api/ffx_fsr2.cpp",
    "src/ffx-fsr2-api/ffx_assert.cpp",
    "src/ffx-fsr2-api/dx11/ffx_fsr2_dx11.cpp",
    "src/ffx-fsr2-api/dx11/shaders/ffx_fsr2_shaders_dx11.cpp",
];

fn main() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let generated = crate_dir.join("generated/dx11");
    let sentinel = crate_dir.join(SENTINEL_HEADER);
    println!("cargo:rerun-if-changed={}", sentinel.display());

    if !sentinel.exists() {
        panic!(
            "fsr-sys: generated FSR2 shader headers are missing ({SENTINEL_HEADER}).\n       \
             Generate them once with:\n           \
             cargo run -p fsr-shadergen --target x86_64-unknown-linux-gnu\n       \
             (see fsr-sys/README.md). They are git-ignored while the integration is in flux."
        );
    }

    let vendor = vendor_dir(crate_dir);
    for src in SOURCES {
        println!("cargo:rerun-if-changed={}", vendor.join(src).display());
    }

    let shim = crate_dir.join("shim/fsr_shim.cpp");
    println!("cargo:rerun-if-changed={}", shim.display());
    println!(
        "cargo:rerun-if-changed={}",
        crate_dir.join("shim/fsr_shim.h").display()
    );

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        // The backend uses the C++17-deprecated <codecvt> for UTF-8/16 conversion; it still works,
        // so silence the deprecation rather than patch vendored code.
        .define("_SILENCE_CXX17_CODECVT_HEADER_DEPRECATION_WARNING", None)
        // The DX11 shader backend `#include`s the permutation headers by bare name.
        .include(&generated)
        .include(vendor.join("src/ffx-fsr2-api"));
    for src in SOURCES {
        build.file(vendor.join(src));
    }
    // Our C shim sits alongside the backend and includes its headers (ffx_fsr2.h, dx11/...).
    build.file(&shim);
    build.compile("ffx_fsr2_dx11");

    // The backend pulls in the D3D11 + shader-compiler import libs.
    println!("cargo:rustc-link-lib=d3d11");
    println!("cargo:rustc-link-lib=d3dcompiler");
    println!("cargo:rustc-link-lib=dxguid");
}

/// Locate the vendored submodule (workspace-root `vendor/`), overridable via `FSR_VENDOR_DIR`.
fn vendor_dir(crate_dir: &Path) -> PathBuf {
    std::env::var_os("FSR_VENDOR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("../vendor/FidelityFX-FSR2-DX11"))
}
