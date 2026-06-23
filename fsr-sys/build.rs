//! Compiles the vendored FSR2 DX11 backend and links it into the crate.
//!
//! The shader-permutation headers under `generated/dx11/` are git-ignored (124 files, ~9 MB), but a
//! committed `generated/dx11.tar.gz` (~0.7 MB) holds them. When the headers are missing -- a fresh
//! checkout or CI -- we unpack the archive, so the build needs no shader compiler. If neither is
//! present, the `shadergen` crate regenerates both.

use std::path::{Path, PathBuf};

/// One representative generated header; its absence means the set needs unpacking (or regenerating).
const SENTINEL_HEADER: &str = "generated/dx11/ffx_fsr2_rcas_pass_permutations.h";

/// The committed compressed archive of the generated headers, unpacked when they're missing.
const HEADERS_ARCHIVE: &str = "generated/dx11.tar.gz";

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
    let archive = crate_dir.join(HEADERS_ARCHIVE);
    println!("cargo:rerun-if-changed={}", sentinel.display());
    println!("cargo:rerun-if-changed={}", archive.display());

    if !sentinel.exists() {
        unpack_headers(&archive, &crate_dir.join("generated"));
    }
    if !sentinel.exists() {
        panic!(
            "fsr-sys: generated FSR2 shader headers are missing and {HEADERS_ARCHIVE} did not \
             unpack them.\n       Regenerate both with:\n           \
             cargo run -p shadergen --target x86_64-unknown-linux-gnu\n       \
             (see fsr-sys/README.md)."
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

/// Unpack the committed header archive into `dest` (the `generated/` dir; the archive holds a `dx11/`
/// entry). No-op if the archive is absent -- the caller then falls back to the regenerate instruction.
fn unpack_headers(archive: &Path, dest: &Path) {
    let Ok(file) = std::fs::File::open(archive) else {
        return;
    };
    let decoder = flate2::read::GzDecoder::new(file);
    if let Err(e) = tar::Archive::new(decoder).unpack(dest) {
        panic!("fsr-sys: failed to unpack {}: {e}", archive.display());
    }
}

/// Locate the vendored submodule (workspace-root `vendor/`), overridable via `FSR_VENDOR_DIR`.
fn vendor_dir(crate_dir: &Path) -> PathBuf {
    std::env::var_os("FSR_VENDOR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("../vendor/FidelityFX-FSR2-DX11"))
}
