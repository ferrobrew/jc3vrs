//! Regenerate the FSR2 DX11 compute-shader permutation headers consumed by `fsr-sys`.
//!
//! The FidelityFX DX11 backend bakes its compute shaders in as DXBC bytecode: the shader compiler
//! (`FidelityFX_SC.exe`, bundled in the vendored submodule) compiles each pass into a
//! `<pass>_permutations.h` header of byte arrays, which `ffx_fsr2_shaders_dx11.cpp` `#include`s. Those
//! generated headers land under `fsr-sys/generated/` (git-ignored for now); `fsr-sys`'s `build.rs`
//! errors if they're missing, pointing here. This is its own crate -- separate from `fsr-sys` -- so
//! its HTTP / archive dependencies never reach a normal build of the `-sys` crate or the payload. It
//! runs only on an FSR version bump or after a fresh checkout.
//!
//! `FidelityFX_SC.exe` is a Windows executable: on a Windows host it runs directly; elsewhere it runs
//! under Wine. The Wine half -- provisioning a prefix with a native `d3dcompiler_47.dll`, since Wine's
//! built-in reimplementation rejects FSR's shaders -- is isolated in [`wine`]. The compile recipe in
//! this file matches the upstream CMake (`src/ffx-fsr2-api/CMakeLists.txt` base args +
//! `src/ffx-fsr2-api/dx11/CMakeLists.txt` DX11 args), the same on both hosts.
//!
//! Usage: `cargo run -p fsr-shadergen`. On a non-Windows host this also needs `wine` on PATH (from
//! `shell.nix`) and network access on the first run to fetch the native DLL.
//!
//! Environment overrides (all optional):
//! - `FFX_SC`             -- override the path to `FidelityFX_SC.exe` (default: the vendored copy).
//! - `FSR_VENDOR_DIR`     -- override the submodule root (default: `vendor/FidelityFX-FSR2-DX11`).
//! - `FSR_GENERATED_DIR`  -- override the output dir (default: `fsr-sys/generated/dx11`).
//! - `WINE`               -- the Wine binary, when running under Wine (default `wine`).
//! - `WINEPREFIX`         -- use this prefix instead of the managed one under `target/`.
//! - `D3DCOMPILER_DLL`    -- use this native `d3dcompiler_47.dll` instead of downloading one.

mod wine;

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use wine::WinePrefix;

/// The eight FSR2 pass shaders, in the upstream build's order. The `bool` is whether a 16-bit
/// (`FFX_HALF=1`) permutation is also built: every pass except the luminance pyramid has one.
const PASSES: &[(&str, bool)] = &[
    ("ffx_fsr2_tcr_autogen_pass", true),
    ("ffx_fsr2_autogen_reactive_pass", true),
    ("ffx_fsr2_accumulate_pass", true),
    ("ffx_fsr2_compute_luminance_pyramid_pass", false),
    ("ffx_fsr2_depth_clip_pass", true),
    ("ffx_fsr2_lock_pass", true),
    ("ffx_fsr2_reconstruct_previous_depth_pass", true),
    ("ffx_fsr2_rcas_pass", true),
];

/// SDK-level base args (`FFX_SC_BASE_ARGS` in `ffx-fsr2-api/CMakeLists.txt`).
const SDK_BASE_ARGS: &[&str] = &[
    "-reflection",
    "-deps=gcc",
    "-DFFX_GPU=1",
    "-DFFX_FSR2_OPTION_UPSAMPLE_SAMPLERS_USE_DATA_HALF=0",
    "-DFFX_FSR2_OPTION_ACCUMULATE_SAMPLERS_USE_DATA_HALF=0",
    "-DFFX_FSR2_OPTION_REPROJECT_SAMPLERS_USE_DATA_HALF=1",
    "-DFFX_FSR2_OPTION_POSTPROCESSLOCKSTATUS_SAMPLERS_USE_DATA_HALF=0",
    "-DFFX_FSR2_OPTION_UPSAMPLE_USE_LANCZOS_TYPE=2",
];

/// Permutation args (`FFX_SC_PERMUTATION_ARGS`): the `{0,1}` sets fan out into every combination.
const SDK_PERMUTATION_ARGS: &[&str] = &[
    "-DFFX_FSR2_OPTION_REPROJECT_USE_LANCZOS_TYPE={0,1}",
    "-DFFX_FSR2_OPTION_HDR_COLOR_INPUT={0,1}",
    "-DFFX_FSR2_OPTION_LOW_RESOLUTION_MOTION_VECTORS={0,1}",
    "-DFFX_FSR2_OPTION_JITTERED_MOTION_VECTORS={0,1}",
    "-DFFX_FSR2_OPTION_INVERTED_DEPTH={0,1}",
    "-DFFX_FSR2_OPTION_APPLY_SHARPENING={0,1}",
];

/// DX11 backend base args (`FFX_SC_DX11_BASE_ARGS` in `dx11/CMakeLists.txt`).
const DX11_BASE_ARGS: &[&str] = &[
    "-E",
    "CS",
    "-DFFX_HLSL=1",
    "-DFFX_HLSL_5_0=1",
    "-compiler=fxc",
    "-DSPD_NO_WAVE_OPERATIONS",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("regen-shaders: {e}");
        std::process::exit(1);
    }
}

/// How `FidelityFX_SC.exe` is invoked: directly on a Windows host, or through Wine elsewhere.
enum Runner {
    Native,
    Wine(WinePrefix),
}

impl Runner {
    /// Build the command to run `sc_exe` with `args`, applying the Wine wrapping when needed.
    fn command(&self, sc_exe: &Path, args: &[String]) -> Command {
        match self {
            Runner::Native => {
                let mut cmd = Command::new(sc_exe);
                cmd.args(args);
                cmd
            }
            Runner::Wine(prefix) => {
                let mut cmd = Command::new(&prefix.wine);
                cmd.arg(sc_exe).args(args);
                prefix.apply(&mut cmd);
                cmd
            }
        }
    }
}

fn run() -> Result<(), String> {
    // This crate sits at the workspace root next to `fsr-sys` and `vendor/`; resolve those siblings
    // relative to its manifest dir.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let vendor =
        env_path("FSR_VENDOR_DIR").unwrap_or_else(|| workspace.join("vendor/FidelityFX-FSR2-DX11"));
    let sc_exe = env_path("FFX_SC").unwrap_or_else(|| vendor.join("tools/sc/FidelityFX_SC.exe"));
    let shader_src = vendor.join("src/ffx-fsr2-api/shaders");
    let out_dir =
        env_path("FSR_GENERATED_DIR").unwrap_or_else(|| workspace.join("fsr-sys/generated/dx11"));

    if !sc_exe.exists() {
        return Err(format!(
            "FidelityFX_SC.exe not found at {} -- run `git submodule update --init` first",
            sc_exe.display()
        ));
    }

    // On Windows the compiler runs natively; elsewhere it runs through a provisioned Wine prefix.
    let runner = if cfg!(windows) {
        println!("regen-shaders: compiling {} passes natively", PASSES.len());
        Runner::Native
    } else {
        let work = workspace.join("target/fsr-shader-build");
        let prefix = WinePrefix::provision(&work)?;
        println!("regen-shaders: compiling {} passes via Wine", PASSES.len());
        Runner::Wine(prefix)
    };

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("could not create {}: {e}", out_dir.display()))?;

    for (pass, has_16bit) in PASSES {
        compile_pass(&runner, &sc_exe, &shader_src, &out_dir, pass, false)?;
        if *has_16bit {
            compile_pass(&runner, &sc_exe, &shader_src, &out_dir, pass, true)?;
        }
    }
    prune_depfiles(&out_dir)?;
    println!("regen-shaders: done -> {}", out_dir.display());
    Ok(())
}

/// Compile one pass permutation (`half` selects the `FFX_HALF=1` / 16-bit variant), matching the
/// per-shader `add_custom_command` the upstream CMake emits.
fn compile_pass(
    runner: &Runner,
    sc_exe: &Path,
    shader_src: &Path,
    out_dir: &Path,
    pass: &str,
    half: bool,
) -> Result<(), String> {
    let name = if half {
        format!("{pass}_16bit")
    } else {
        pass.to_string()
    };
    let half_def = if half { "-DFFX_HALF=1" } else { "-DFFX_HALF=0" };

    let mut args: Vec<String> = Vec::new();
    args.extend(SDK_BASE_ARGS.iter().map(|s| s.to_string()));
    args.extend(DX11_BASE_ARGS.iter().map(|s| s.to_string()));
    args.extend(SDK_PERMUTATION_ARGS.iter().map(|s| s.to_string()));
    args.push(format!("-name={name}"));
    args.push(half_def.to_string());
    args.push("-T".to_string());
    args.push("cs_5_0".to_string());
    // RCAS compiles without optimization on purpose (faster at runtime); the rest optimize normally.
    if pass == "ffx_fsr2_rcas_pass" {
        args.push("-Od".to_string());
    }
    args.push("-I".to_string());
    args.push(shader_src.to_string_lossy().into_owned());
    args.push(format!("-output={}", out_dir.to_string_lossy()));
    args.push(
        shader_src
            .join(format!("{pass}.hlsl"))
            .to_string_lossy()
            .into_owned(),
    );

    let status = runner
        .command(sc_exe, &args)
        .status()
        .map_err(|e| format!("failed to spawn the shader compiler: {e}"))?;
    if !status.success() {
        return Err(format!("shader compile failed for {name} (exit {status})"));
    }
    println!("  compiled {name}");
    Ok(())
}

/// Remove the GCC-style `.d` depfiles the compiler emits alongside each header (`-deps=gcc`). They
/// are build-system scaffolding and carry absolute host paths, so they don't belong in the output.
fn prune_depfiles(out_dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(out_dir)
        .map_err(|e| format!("could not read {}: {e}", out_dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "d") {
            std::fs::remove_file(&path)
                .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}
