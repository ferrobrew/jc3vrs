//! Compiles the vendored FSR2 DX11 backend and links it into the crate.
//!
//! The shader-permutation headers under `generated/dx11/` are produced by the `fsr-shadergen` crate
//! (and git-ignored for now), so a fresh checkout must run it once before this builds. We check for
//! them up front and fail with that instruction rather than emitting a wall of missing-include errors
//! from the C++ compiler.
//!
//! The actual C++ compile (the four backend `.cpp` files via `cc`, with `generated/dx11` on the
//! include path) is not wired up yet -- this is the crate skeleton.

use std::path::Path;

/// One representative generated header; its absence means the whole set needs regenerating.
const SENTINEL_HEADER: &str = "generated/dx11/ffx_fsr2_rcas_pass_permutations.h";

fn main() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
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
}
