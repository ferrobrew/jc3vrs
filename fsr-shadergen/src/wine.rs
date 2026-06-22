//! Wine provisioning for the shader regenerator, isolated from the build steps.
//!
//! On a non-Windows host the shader compiler runs under Wine, and its `-compiler=fxc` path needs a
//! *native* `d3dcompiler_47.dll` (Wine's built-in reimplementation rejects FSR's shaders). This
//! module owns everything that entails: downloading the native DLL, extracting it from the Firefox
//! redist (a 7-zip self-extracting installer), initializing a managed Wine prefix, and installing the
//! DLL into it. The build steps in `main.rs` only ask for a ready-to-use [`WinePrefix`].

use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
};

/// A Wine prefix provisioned with a native `d3dcompiler_47.dll`, ready to run `FidelityFX_SC.exe`.
pub struct WinePrefix {
    pub wine: String,
    pub prefix: PathBuf,
}

/// A Firefox release ships a redistributable native `d3dcompiler_47.dll`; this is the standard
/// winetricks source for it. The DLL lives at `core/d3dcompiler_47.dll` inside the installer.
const D3DCOMPILER_INSTALLER_URL: &str = "https://download-installer.cdn.mozilla.net/pub/firefox/releases/62.0.3/win64/en-US/Firefox%20Setup%2062.0.3.exe";
const D3DCOMPILER_MEMBER: &str = "core/d3dcompiler_47.dll";
/// 7z archive signature (`7z\xBC\xAF\x27\x1C`); the Firefox installer is a PE stub followed by the
/// 7z archive, so we scan for this to find where the archive begins.
const SEVENZ_MAGIC: &[u8] = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C];

impl WinePrefix {
    /// Provision a Wine prefix with the native d3dcompiler. `work` is a cache dir (under the
    /// git-ignored `target/`) holding the downloaded DLL and the prefix across runs.
    pub fn provision(work: &Path) -> Result<Self, String> {
        let wine = std::env::var("WINE").unwrap_or_else(|_| "wine".to_string());
        let prefix = super::env_path("WINEPREFIX").unwrap_or_else(|| work.join("wineprefix"));
        let dll = obtain_d3dcompiler(work)?;
        install_into_prefix(&wine, &prefix, &dll)?;
        Ok(WinePrefix { wine, prefix })
    }

    /// Configure `cmd` (already targeting `wine`) to run against this prefix with the native
    /// d3dcompiler override.
    pub fn apply(&self, cmd: &mut Command) {
        cmd.env("WINEPREFIX", &self.prefix)
            .env("WINEDEBUG", "-all")
            // Force the native d3dcompiler_47 over Wine's built-in (which mis-parses FSR's shaders).
            .env("WINEDLLOVERRIDES", "d3dcompiler_47=n");
    }
}

/// Obtain a native `d3dcompiler_47.dll`, caching it under `work/`. Honors `D3DCOMPILER_DLL` if set;
/// otherwise downloads the Firefox redist and extracts the DLL once (both in-process).
fn obtain_d3dcompiler(work: &Path) -> Result<PathBuf, String> {
    if let Some(p) = super::env_path("D3DCOMPILER_DLL") {
        if !p.exists() {
            return Err(format!(
                "D3DCOMPILER_DLL points at a missing file: {}",
                p.display()
            ));
        }
        return Ok(p);
    }

    let cached = work.join("d3dcompiler_47.dll");
    if cached.exists() {
        return Ok(cached);
    }
    std::fs::create_dir_all(work)
        .map_err(|e| format!("could not create {}: {e}", work.display()))?;

    println!("regen-shaders: downloading native d3dcompiler_47.dll (once)");
    let installer = download(D3DCOMPILER_INSTALLER_URL)?;
    println!("regen-shaders: extracting {D3DCOMPILER_MEMBER}");
    let dll = extract_member(&installer, D3DCOMPILER_MEMBER)?;
    std::fs::write(&cached, dll).map_err(|e| {
        format!(
            "could not cache the d3dcompiler at {}: {e}",
            cached.display()
        )
    })?;
    Ok(cached)
}

/// Blocking HTTPS GET into a byte buffer.
fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    ureq::get(url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?
        .body_mut()
        .as_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("reading the download body failed: {e}"))?;
    Ok(body)
}

/// Extract a single named member from a 7z self-extracting installer held in memory. The installer
/// is a PE stub followed by the 7z archive, so we locate the archive signature and read from there.
fn extract_member(installer: &[u8], member: &str) -> Result<Vec<u8>, String> {
    let start = find_subslice(installer, SEVENZ_MAGIC)
        .ok_or("could not find the 7z archive inside the installer (layout may have changed)")?;
    let archive = &installer[start..];

    let mut reader =
        sevenz_rust2::ArchiveReader::new(Cursor::new(archive), sevenz_rust2::Password::empty())
            .map_err(|e| format!("opening the 7z archive failed: {e}"))?;
    reader
        .read_file(member)
        .map_err(|e| format!("extracting {member} failed: {e}"))
}

/// Create the Wine prefix if needed and copy the native DLL into its `system32` (overriding Wine's
/// built-in). Idempotent: safe to re-run.
fn install_into_prefix(wine: &str, prefix: &Path, dll: &Path) -> Result<(), String> {
    let system32 = prefix.join("drive_c/windows/system32");
    if !system32.exists() {
        println!(
            "regen-shaders: initializing Wine prefix at {}",
            prefix.display()
        );
        let status = Command::new(wine)
            .arg("wineboot")
            .arg("--init")
            .env("WINEPREFIX", prefix)
            .env("WINEDEBUG", "-all")
            .status()
            .map_err(|e| format!("failed to spawn {wine} (is it on PATH?): {e}"))?;
        if !status.success() {
            return Err(format!("wineboot failed (exit {status})"));
        }
    }
    std::fs::create_dir_all(&system32)
        .map_err(|e| format!("could not create {}: {e}", system32.display()))?;
    let dest = system32.join("d3dcompiler_47.dll");
    std::fs::copy(dll, &dest)
        .map_err(|e| format!("could not install d3dcompiler into the prefix: {e}"))?;
    Ok(())
}

/// First index of `needle` within `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
