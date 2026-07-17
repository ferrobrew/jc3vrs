//! Regenerate the FSR2 DX11 compute-shader permutation headers consumed by `fsr-sys`, and compile the
//! payload's own DX11 shaders.
//!
//! The FidelityFX DX11 backend bakes its compute shaders in as DXBC bytecode: the shader compiler
//! (`FidelityFX_SC.exe`, bundled in the vendored submodule) compiles each pass into a
//! `<pass>_permutations.h` header of byte arrays, which `ffx_fsr2_shaders_dx11.cpp` `#include`s. Those
//! generated headers land under `fsr-sys/generated/` (git-ignored for now); `fsr-sys`'s `build.rs`
//! errors if they're missing, pointing here. This is its own crate -- separate from `fsr-sys` -- so
//! its HTTP / archive dependencies never reach a normal build of the `-sys` crate or the payload. It
//! runs only on an FSR version bump or after a fresh checkout.
//!
//! In addition, the payload's own DX11 shaders (velocity decode, HUD quad) are compiled by the same
//! toolchain and committed as `.dxbc` blobs alongside their `.hlsl` sources.
//!
//! `FidelityFX_SC.exe` is a Windows executable: on a Windows host it runs directly; elsewhere it runs
//! under Wine. The Wine half -- provisioning a prefix with a native `d3dcompiler_47.dll`, since Wine's
//! built-in reimplementation rejects FSR's shaders -- is isolated in [`wine`]. The compile recipe in
//! this file matches the upstream CMake (`src/ffx-fsr2-api/CMakeLists.txt` base args +
//! `src/ffx-fsr2-api/dx11/CMakeLists.txt` DX11 args), the same on both hosts.
//!
//! Usage: `cargo run -p shadergen`. On a non-Windows host this also needs `wine` on PATH (from
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

/// The payload's own shaders: `(name, shader-model profile)`. Each `payload/src/shaders/<name>.hlsl`
/// (entry point `main`) compiles to a committed `payload/src/shaders/<name>.dxbc`.
const PAYLOAD_SHADERS: &[(&str, &str)] = &[
    ("velocity_decode", "cs_5_0"),
    ("depth_histogram_cs", "cs_5_0"),
    ("hud_quad_vs", "vs_5_0"),
    ("hud_layer_vs", "vs_5_0"),
    ("hud_quad_ps", "ps_5_0"),
    ("cursor_ps", "ps_5_0"),
    ("capture_vs", "vs_5_0"),
    ("capture_ps", "ps_5_0"),
    ("vr_blit_ps", "ps_5_0"),
    ("foveation_mask_ps", "ps_5_0"),
    ("foveation_fill_ps", "ps_5_0"),
    ("far_field_composite_ps", "ps_5_0"),
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
    canonicalize_permutation_indexes(&out_dir)?;
    archive_headers(&out_dir)?;
    println!("regen-shaders: done -> {}", out_dir.display());

    // The payload's own shaders use the same toolchain. Each compiles to a committed `.dxbc` blob the
    // payload `include_bytes!`s -- no runtime shader compiler (uncertain under Proton).
    let payload_shaders = workspace.join("payload/src/shaders");
    for (name, profile) in PAYLOAD_SHADERS {
        compile_payload_shader(&runner, &sc_exe, &payload_shaders, name, profile)?;
    }
    println!(
        "regen-shaders: payload shaders -> {}",
        payload_shaders.display()
    );
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

/// Compile one payload shader (`<dir>/<name>.hlsl`, entry point `main`) to a committed
/// `<dir>/<name>.dxbc` blob. Uses the same compiler but emits the SC permutation header into a temp
/// dir, then lifts the raw DXBC bytes out of it -- the payload wants a plain blob to `include_bytes!`,
/// not FSR's permutation-table header.
fn compile_payload_shader(
    runner: &Runner,
    sc_exe: &Path,
    dir: &Path,
    name: &str,
    profile: &str,
) -> Result<(), String> {
    let tmp = dir.join(".sc-tmp");
    std::fs::create_dir_all(&tmp)
        .map_err(|e| format!("could not create {}: {e}", tmp.display()))?;

    let args: Vec<String> = vec![
        "-E".into(),
        "main".into(),
        "-T".into(),
        profile.into(),
        "-compiler=fxc".into(),
        "-DFFX_HLSL=1".into(),
        format!("-name={name}"),
        format!("-output={}", tmp.to_string_lossy()),
        dir.join(format!("{name}.hlsl"))
            .to_string_lossy()
            .into_owned(),
    ];
    let status = runner
        .command(sc_exe, &args)
        .status()
        .map_err(|e| format!("failed to spawn the shader compiler: {e}"))?;
    if !status.success() {
        return Err(format!("shader compile failed for {name} (exit {status})"));
    }

    let blob = lift_dxbc(&tmp, name)?;
    let dest = dir.join(format!("{name}.dxbc"));
    std::fs::write(&dest, blob).map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    std::fs::remove_dir_all(&tmp).ok();
    println!("  compiled {name} -> {}.dxbc", name);
    Ok(())
}

/// Recover the raw DXBC bytes from the SC tool's generated blob header. The header declares a
/// `static const unsigned char g_<hash>_data[] = { 0x.., ... };`; we parse those hex bytes back out.
fn lift_dxbc(tmp: &Path, name: &str) -> Result<Vec<u8>, String> {
    // The blob header is the `<name>_<hash>.h` that is not the `<name>_permutations.h` index.
    let mut blob_header = None;
    for entry in std::fs::read_dir(tmp).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let file = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if file.starts_with(name)
            && file.ends_with(".h")
            && file != format!("{name}_permutations.h")
        {
            blob_header = Some(path);
            break;
        }
    }
    let header = blob_header.ok_or_else(|| format!("no blob header emitted for {name}"))?;
    let text = std::fs::read_to_string(&header).map_err(|e| e.to_string())?;

    // Take everything between the `_data[] = {` and the closing `}` and parse the `0x..` tokens.
    let start = text
        .find("_data[]")
        .and_then(|i| text[i..].find('{').map(|j| i + j + 1))
        .ok_or("could not find the data array in the blob header")?;
    let end = text[start..]
        .find('}')
        .map(|j| start + j)
        .ok_or("unterminated data array in the blob header")?;
    let bytes = text[start..end]
        .split(',')
        .filter_map(|t| {
            let t = t.trim();
            t.strip_prefix("0x")
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        })
        .collect::<Vec<u8>>();
    if bytes.len() < 4 || &bytes[..4] != b"DXBC" {
        return Err(format!("{name}: lifted bytes are not a DXBC blob"));
    }
    Ok(bytes)
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

/// Canonicalize each `<pass>_permutations.h` index so regeneration is reproducible.
/// `FidelityFX_SC.exe` numbers the deduplicated blobs in whatever order it happens to process
/// them, which differs on every run: the `#include` list, the `g_<pass>_PermutationInfo[]` entry
/// order, and the `g_<pass>_IndirectionTable[]` values it holds all shuffle together, even though
/// the content-hashed blob headers themselves are stable. Sorting the includes and the info
/// entries, and remapping the indirection table through the same permutation, preserves the
/// key-to-blob mapping while pinning the bytes.
fn canonicalize_permutation_indexes(out_dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(out_dir)
        .map_err(|e| format!("could not read {}: {e}", out_dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.ends_with("_permutations.h") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let canonical = canonicalize_index(&text)
            .map_err(|e| format!("could not canonicalize {}: {e}", path.display()))?;
        if canonical != text {
            std::fs::write(&path, canonical)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Rewrite one permutation index into its canonical form (see
/// [`canonicalize_permutation_indexes`]). Each `PermutationInfo` entry line embeds its blob's
/// content hash, so sorting the entry lines lexicographically is a stable, content-derived order.
fn canonicalize_index(text: &str) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

    let includes = lines
        .iter()
        .take_while(|l| l.starts_with("#include"))
        .count();
    lines[..includes].sort_unstable();

    let table = block_range(&lines, "_IndirectionTable[] = {")?;
    let info = block_range(&lines, "_PermutationInfo[] = {")?;

    let info_lines = lines[info.clone()].to_vec();
    let mut order: Vec<usize> = (0..info_lines.len()).collect();
    order.sort_by(|&a, &b| info_lines[a].cmp(&info_lines[b]));
    let mut remap = vec![0usize; order.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
        lines[info.start + new] = info_lines[old].clone();
    }

    for line in &mut lines[table] {
        let entry = line.trim().trim_end_matches(',');
        let old: usize = entry
            .parse()
            .map_err(|_| format!("non-numeric IndirectionTable entry `{entry}`"))?;
        let new = remap
            .get(old)
            .ok_or_else(|| format!("IndirectionTable entry {old} is out of range"))?;
        *line = format!("    {new},");
    }

    let mut canonical = lines.join("\n");
    if text.ends_with('\n') {
        canonical.push('\n');
    }
    Ok(canonical)
}

/// Find the body of the array whose declaration line contains `marker`: the lines strictly between
/// the declaration and its closing `};`.
fn block_range(lines: &[String], marker: &str) -> Result<std::ops::Range<usize>, String> {
    let start = lines
        .iter()
        .position(|l| l.contains(marker))
        .ok_or_else(|| format!("missing a `{marker}` declaration"))?
        + 1;
    let len = lines[start..]
        .iter()
        .position(|l| l.starts_with("};"))
        .ok_or_else(|| format!("unterminated `{marker}` array"))?;
    Ok(start..start + len)
}

/// Pack the generated headers into a committed `dx11.tar.gz` next to the `dx11/` dir. The extracted
/// headers are git-ignored (124 files, ~9 MB) but this archive (~0.7 MB) is committed, so a fresh
/// checkout or CI builds without a shader compiler: `fsr-sys`'s `build.rs` unpacks it on demand.
///
/// The archive is byte-reproducible: entries are appended in sorted order with zeroed metadata
/// (mtime, uid/gid, fixed modes) and the gzip header carries no timestamp, so re-running shadergen
/// over unchanged headers leaves the committed archive untouched.
fn archive_headers(out_dir: &Path) -> Result<(), String> {
    let archive_path = out_dir.with_file_name("dx11.tar.gz");
    let file = std::fs::File::create(&archive_path)
        .map_err(|e| format!("could not create {}: {e}", archive_path.display()))?;
    let encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(file, flate2::Compression::best());
    let mut tar = tar::Builder::new(encoder);

    let mut names = Vec::new();
    let entries = std::fs::read_dir(out_dir)
        .map_err(|e| format!("could not read {}: {e}", out_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| format!("non-UTF-8 file name in {}: {name:?}", out_dir.display()))?
            .to_string();
        names.push(name);
    }
    names.sort_unstable();

    // Store entries under `dx11/` so extraction into the parent recreates the headers directory.
    let mut dir_header = tar::Header::new_gnu();
    dir_header.set_entry_type(tar::EntryType::Directory);
    dir_header.set_mode(0o755);
    dir_header.set_size(0);
    dir_header.set_mtime(0);
    tar.append_data(&mut dir_header, "dx11", std::io::empty())
        .map_err(|e| format!("could not archive the dx11 directory entry: {e}"))?;
    for name in &names {
        let path = out_dir.join(name);
        let data =
            std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(data.len() as u64);
        header.set_mtime(0);
        tar.append_data(&mut header, format!("dx11/{name}"), data.as_slice())
            .map_err(|e| format!("could not archive {}: {e}", path.display()))?;
    }

    tar.into_inner()
        .and_then(|enc| enc.finish())
        .map_err(|e| format!("could not finish {}: {e}", archive_path.display()))?;
    println!("regen-shaders: archived -> {}", archive_path.display());
    Ok(())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}
