//! Detour on `Graphics::CreateFragmentProgram` to neutralise the per-eye sun-shadow PCF rotation hash.
//!
//! The opaque sun-shadow resolve rotates its 38-tap Poisson PCF disk by
//! `frac(sin(dot(SV_Position, k)) * 43758.5)` -- a hash of the screen pixel. In stereo the same world
//! point lands on a different pixel in each eye, so the two eyes average a different tap set: the
//! shadow shimmers/grains differently between the eyes (and the alpha-tested foliage with it). The
//! world-space shadow *lookup* uses the interpolated world position and is identical per eye, so the
//! fix is to make the rotation eye-invariant -- zero the two seed constants in that `dp2`, so every
//! pixel (and both eyes) uses the same unrotated 38-tap PCF. With 38 taps the look change is
//! negligible. `12.9898` occurs only in this instruction (159 shaders per bundle), so the patch is
//! exactly targeted.
//!
//! The patch is applied to the DXBC in-flight, before `CreatePixelShader` (which copies the bytecode,
//! so a patched copy only needs to outlive the call). Editing the bytecode invalidates the DXBC
//! container checksum, and the D3D stack under Proton rejects a blob whose stored hash no longer
//! matches -- so the patched copy's checksum is recomputed (`dxbc_stereo::refresh_checksum`) before
//! the call.
//! It therefore affects only shaders created after
//! the hook installs: with launch-time injection that is every shader; with mid-session injection,
//! trigger a shader reload (e.g. change the shadow-quality graphics setting) so the shadow shaders are
//! recreated through the hook. [`patched_count`] (shown in the debug UI) makes it clear whether the
//! hook is catching anything.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use detours_macro::detour;
use dxbc_stereo::refresh_checksum;
use jc3gi::graphics_engine::{
    draw::{CreateFragmentProgramParams, CreateVertexProgramParams},
    graphics_engine::GraphicsEngine,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

/// The 16-byte `dp2` immediate `l(12.9898, 78.233, 0, 0)` -- the screen-pixel PCF rotation seed. The
/// first eight bytes are the two multiplier constants; zeroing them makes the dot product (and thus
/// the rotation angle) a constant.
const SEED: [u8; 16] = [
    0x39, 0xd6, 0x4f, 0x41, // 12.9898
    0x4c, 0x77, 0x9c, 0x42, // 78.233
    0x00, 0x00, 0x00, 0x00, // 0.0
    0x00, 0x00, 0x00, 0x00, // 0.0
];

/// The 8-byte `dp2` immediate prefix `l(0.467944, -0.703648, ...)` -- the material LOD-dissolve
/// screen-door pattern seed (119 shaders per bundle carry it exactly once). Most key the pattern to
/// `SV_Position` (raster pixels, temporally stable); the vegetation family instead keys it to the
/// interpolated clip-space position, which carries the FSR camera jitter -- the whole dissolve
/// pattern then slides sub-pixel every frame and each mid-fade region flips coverage coherently,
/// the blob-scale scene flicker of issue #10. The two families are told apart by the `dp2` source
/// operand: a TEMP register (the perspective-divided interpolant) marks the unstable one.
const DISSOLVE_SEED: [u8; 8] = [
    0x5b, 0x96, 0xef, 0x3e, // 0.467944
    0x46, 0x22, 0x34, 0xbf, // -0.703648
];
/// Offset from the dissolve seed to the fade `add`'s `l(1.0)` immediate (`add r#, -v#.#, l(1.0)`,
/// two instructions after the `dp2`; byte-identical placement across the unstable family).
const DISSOLVE_FADE_OFFSET: usize = 64;
/// `1.0f`, the fade immediate the unstable family carries at [`DISSOLVE_FADE_OFFSET`].
const F32_ONE: [u8; 4] = [0x00, 0x00, 0x80, 0x3f];
/// `-3.0e38f`. Replacing the fade immediate with it drives the dissolve sum hugely positive, so the
/// discard can never fire: the dissolve is disabled and the mesh draws fully (LOD transitions pop
/// instead of dissolving -- stable under jitter).
const DISSOLVE_NEVER: [u8; 4] = [0xe6, 0xb1, 0x61, 0xff];

static PATCHED: AtomicUsize = AtomicUsize::new(0);
static DISSOLVE_PATCHED: AtomicUsize = AtomicUsize::new(0);
static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CREATE_FRAGMENT_PROGRAM_BINDER)
        .with_static_binder(&CREATE_VERTEX_PROGRAM_BINDER)
}

/// Detour on `Graphics::CreateVertexProgram` for single-pass stereo: census the vertex-shader rewrite
/// and, when single-pass is active, substitute the patched bytecode.
///
/// When single-pass (or its census-only dry-run) is enabled, run [`dxbc_stereo::patch_vertex_shader`]
/// on the incoming DXBC and tally the outcome ([`crate::stereo::single_pass::record_patch_outcome`]),
/// so the debug UI reports how the rewriter fares against the game's real shader set. When single-pass
/// is *active* (master on, dry-run off, capability present), also point the params at the patched copy
/// for the (bytecode-copying) `CreateVertexShader` call -- the copy is kept alive in `saved` and the
/// caller's pointer restored afterwards, exactly as the fragment hook does. The patched shader reads
/// its position from `cb13` (see [`crate::hooks::graphics_engine::single_pass`]); in dry-run the copy
/// is discarded and rendering is unchanged. Only sees shaders created after it installs; use the
/// shader-reload button for shaders loaded before injection.
#[detour(address = jc3gi::graphics_engine::draw::CreateVertexProgram_ADDRESS)]
fn create_vertex_program(
    device: *mut c_void,
    params: *mut CreateVertexProgramParams,
) -> *mut c_void {
    // The patched blob is *larger* than the original (added SFI0 chunk, cb13 declaration, prologue,
    // signature entries), so unlike the in-place fragment patch this must repoint `m_Size` as well as
    // `m_Code` -- otherwise the engine hands the D3D stack a truncated container whose chunk table
    // runs past the declared length. Both are restored after the (bytecode-copying) call.
    let mut saved: Option<(*const u8, u64, Vec<u8>)> = None;
    let census = Config::lock_query(|c| c.stereo.single_pass || c.stereo.single_pass_patch_dryrun);
    if census
        && let Some(p) = unsafe { params.as_mut() }
        && !p.m_Code.is_null()
        && p.m_Size >= 4
    {
        let code = unsafe { std::slice::from_raw_parts(p.m_Code, p.m_Size as usize) };
        let outcome = dxbc_stereo::patch_vertex_shader(code);
        crate::stereo::single_pass::record_patch_outcome(&outcome);
        if crate::stereo::single_pass::active()
            && let Ok(patched) = outcome
        {
            let patched_len = patched.len() as u64;
            saved = Some((p.m_Code, p.m_Size, patched));
            p.m_Code = saved.as_ref().expect("just set").2.as_ptr();
            p.m_Size = patched_len;
        }
    }

    let result = CREATE_VERTEX_PROGRAM.get().unwrap().call(device, params);

    if let Some((original_code, original_size, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original_code;
        p.m_Size = original_size;
    }
    result
}

#[detour(address = jc3gi::graphics_engine::draw::CreateFragmentProgram_ADDRESS)]
fn create_fragment_program(
    device: *mut c_void,
    params: *mut CreateFragmentProgramParams,
) -> *mut c_void {
    // When enabled and a target site is present, point the params at a patched copy of the bytecode
    // for the duration of the (bytecode-copying) CreatePixelShader call, then restore the caller's
    // pointer. `saved` keeps the patched copy alive across the call.
    let mut saved: Option<(*const u8, Vec<u8>)> = None;
    let (patch_pcf, patch_dissolve) =
        Config::lock_query(|c| (c.stereo.patch_shadow_pcf_hash, c.stereo.patch_lod_dissolve));
    if (patch_pcf || patch_dissolve)
        && let Some(p) = unsafe { params.as_mut() }
    {
        let size = p.m_Size as usize;
        if !p.m_Code.is_null() && size >= SEED.len() {
            let code = unsafe { std::slice::from_raw_parts(p.m_Code, size) };
            let mut copy = code.to_vec();
            let pcf = if patch_pcf { zero_seeds(&mut copy) } else { 0 };
            let dissolve = if patch_dissolve {
                neutralize_unstable_dissolves(&mut copy)
            } else {
                0
            };
            if pcf + dissolve > 0 {
                // A raw byte-patch leaves the DXBC container checksum stale; D3D consumers that
                // validate it (the translation layers under Proton do) reject the blob, so the
                // shadow shaders fail to create and the scene renders broken. Refresh the checksum
                // so the patched bytecode is a valid container.
                refresh_checksum(&mut copy);
                PATCHED.fetch_add(pcf, Ordering::Relaxed);
                DISSOLVE_PATCHED.fetch_add(dissolve, Ordering::Relaxed);
                saved = Some((p.m_Code, copy));
                p.m_Code = saved.as_ref().expect("just set").1.as_ptr();
            }
        }
    }

    let result = CREATE_FRAGMENT_PROGRAM.get().unwrap().call(device, params);

    if let Some((original, _copy)) = saved
        && let Some(p) = unsafe { params.as_mut() }
    {
        p.m_Code = original;
    }
    result
}

/// Disable the jitter-unstable LOD dissolve: for each [`DISSOLVE_SEED`] site whose `dp2` reads a
/// TEMP register (the clip-interpolant family; the stable `SV_Position` family reads an INPUT), the
/// fade immediate at [`DISSOLVE_FADE_OFFSET`] is replaced with [`DISSOLVE_NEVER`], making the
/// dissolve's discard unreachable. Returns the number of sites patched.
fn neutralize_unstable_dissolves(code: &mut [u8]) -> usize {
    let mut count = 0;
    let mut i = 12;
    while i + DISSOLVE_FADE_OFFSET + 4 <= code.len() {
        if code[i..i + DISSOLVE_SEED.len()] != DISSOLVE_SEED {
            i += 1;
            continue;
        }
        // The `dp2` source operand token precedes its immediate operand: [token, index] at i-12,
        // with the operand type in bits 12..20 (0 = TEMP, 1 = INPUT).
        let token = u32::from_le_bytes(code[i - 12..i - 8].try_into().expect("4 bytes"));
        let fade = i + DISSOLVE_FADE_OFFSET;
        if (token >> 12) & 0xFF == 0 && code[fade..fade + 4] == F32_ONE {
            code[fade..fade + 4].copy_from_slice(&DISSOLVE_NEVER);
            count += 1;
        }
        i += DISSOLVE_SEED.len();
    }
    count
}

/// Zero the two seed constants (the first eight bytes of each `l(12.9898, 78.233, 0, 0)` immediate),
/// collapsing the PCF disk rotation to a constant angle. Returns the number of sites patched.
fn zero_seeds(code: &mut [u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + SEED.len() <= code.len() {
        if code[i..i + SEED.len()] == SEED {
            code[i..i + 8].fill(0);
            count += 1;
            i += SEED.len();
        } else {
            i += 1;
        }
    }
    count
}

/// The number of PCF-seed sites patched since injection. Surfaced in the debug UI so it is obvious
/// whether the hook is catching shadow shaders -- `0` means none were (re)created after it installed,
/// so a shader reload is needed.
pub fn patched_count() -> usize {
    PATCHED.load(Ordering::Relaxed)
}

/// The number of jitter-unstable LOD-dissolve sites patched since injection. Surfaced in the debug
/// UI alongside [`patched_count`] for the same is-the-hook-catching-anything visibility.
pub fn dissolve_patched_count() -> usize {
    DISSOLVE_PATCHED.load(Ordering::Relaxed)
}

/// Request a shader reload (from the debug UI). Performed at the next [`process_reload_request`] on the
/// game thread, so the patch can be applied to shaders that were created before the hook installed
/// (the usual case -- injection is after the game has loaded its shaders).
pub fn request_reload() {
    RELOAD_REQUESTED.store(true, Ordering::Relaxed);
}

/// If a reload was requested, force the engine to re-create every shader: read the active bundle name,
/// then `LoadShaderBundle` the *other* quality variant and back. Each call re-creates all shader
/// holders through [`create_fragment_program`], so the PCF patch lands. Call once per frame on the game
/// thread (no draw in flight).
pub fn process_reload_request() {
    if !RELOAD_REQUESTED.swap(false, Ordering::Relaxed) {
        return;
    }
    // SAFETY: runs on the game thread at frame start; the engine singleton is live and its
    // `m_CurrentBundleName` is a stable `std::string`. `LoadShaderBundle` is what the settings path
    // calls; we drain the draw first so no GPU work references the shaders being replaced.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let size = ge.m_CurrentBundleName.size;
        if size == 0 || size > 64 {
            tracing::warn!("shader reload: unexpected bundle-name length {size}; skipping");
            return;
        }
        let current = ge.m_CurrentBundleName.as_bytes().to_vec();
        let Ok(current_name) = std::str::from_utf8(&current) else {
            tracing::warn!("shader reload: bundle name is not UTF-8; skipping");
            return;
        };
        let other = toggle_bundle(current_name);

        let mut away = other.as_bytes().to_vec();
        away.push(0);
        let mut back = current.clone();
        back.push(0);

        ge.WaitForCPUDrawToFinish();
        ge.LoadShaderBundle(away.as_ptr());
        // A reload bounces the bundle (away, then back), re-creating every shader through the hooks
        // twice. Reset the single-pass census after the throwaway `away` pass so the reported numbers
        // reflect exactly one clean pass over the real (`back`) shader set.
        crate::stereo::single_pass::reset_census();
        ge.LoadShaderBundle(back.as_ptr());
        tracing::info!(
            "shader reload: '{current_name}' (bounced via '{other}'); {} PCF sites patched total",
            patched_count(),
        );
    }
}

/// The opposite shadow-quality variant of a shader bundle, used to force a reload by switching away and
/// back. Bundles come in `*LowShadows` / non-`*LowShadows` pairs (plus the Intel `ConstMath` variants);
/// toggling the suffix keeps the math variant correct.
fn toggle_bundle(name: &str) -> &'static str {
    match name {
        "Shaders" => "ShadersLowShadows",
        "ShadersLowShadows" => "Shaders",
        "ShadersConstMath" => "ShadersConstMathLowShadows",
        "ShadersConstMathLowShadows" => "ShadersConstMath",
        _ => "ShadersLowShadows",
    }
}
