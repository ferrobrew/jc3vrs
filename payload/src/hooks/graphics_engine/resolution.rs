//! Raise the resolution of the engine's inherently reduced-resolution volumetric passes, whose coarse
//! grids VR's wide field of view magnifies into the blocky "pixelation"/large-tile artifact around
//! lights and explosions (issue #8). Three independent levers, each behind its own config toggle and
//! each **off by default**: they cannot be verified in-headset from native tooling, and one can hide
//! content, so they must be opt-in and inert when off. This is *not* a stereo-reconstruction fix (the
//! per-eye matrices are already correct); it raises the froxel/particle/cone resolutions directly.
//!
//! 1. **Fog volume** ([`fog_volume_resize_textures`]): the froxel volumetric-fog block recreates its
//!    coarse volumetric-depth buffer at half of the full render resolution. Its
//!    `ResizeTextures` halves the width and height with two `mulss xmm0, [0.5]` instructions; while the
//!    toggle is on, both are no-op'd around the call so the coarse buffer is recreated at full
//!    resolution, touching nothing else (the full-res colour and volume textures are unchanged).
//!    Because `ResizeTextures` only runs when the fog textures are recreated (a resolution change), the
//!    toggle takes effect at the next resolution change, not immediately.
//! 2. **Low-res particles** ([`lr_particle_render_pass_draw`]): each particle draw is routed to the
//!    low-resolution particle pass or the full-resolution transparent pass by the particle block
//!    type's [`m_LowResRendering`](RenderBlockTypeParticle::m_LowResRendering)/
//!    [`m_ForceLowResRendering`](RenderBlockTypeParticle::m_ForceLowResRendering) flags. Clearing both
//!    routes every particle to the full-resolution transparent pass (which always draws, so — unlike
//!    disabling the low-res pass's compositing — particles do not vanish). Applied one frame ahead (the
//!    routing runs during the sim add-to-render, before this render-time pass draws) and reverted when
//!    the toggle turns off.
//! 3. **Spotlight volumetrics** ([`copy_lights_to_update`]): the per-frame light gather routes the
//!    volumetric spot-light cone through a quarter-resolution render setup when
//!    [`enable_low_res_spot_light_volume`] is set. The toggle scopes that global to `false` around the
//!    gather, so the engine's own full-resolution branch runs (main render setup, cone block type's
//!    low-res flag cleared) — the lowest-risk lever.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::{
    light_manager::{LightManager, get_enable_low_res_spot_light_volume},
    render_block::{RenderBlockTypeFogVolume, RenderBlockTypeParticle},
    render_pass::LRParticleRenderPass,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&FOG_VOLUME_RESIZE_TEXTURES_BINDER)
        .with_static_binder(&LR_PARTICLE_RENDER_PASS_DRAW_BINDER)
        .with_static_binder(&COPY_LIGHTS_TO_UPDATE_BINDER)
}

// One of the two `mulss xmm0, cs:[0.5]` sites in `RenderBlockTypeFogVolume::ResizeTextures` that halve
// the coarse volumetric-depth buffer's width/height. The RIP-relative displacement differs per site,
// so each site's exact 8-byte encoding is recorded for the sanity check.
struct FogHalveSite {
    address: usize,
    original: [u8; 8],
}

// `mulss xmm0, dword cs:0x1422FE254` (the 0.5 constant) at each halving site; verified before patching.
const FOG_HALVE_SITES: [FogHalveSite; 2] = [
    FogHalveSite {
        address: 0x1_4010_C7DF,
        original: [0xF3, 0x0F, 0x59, 0x05, 0x6D, 0x1A, 0x1F, 0x02],
    },
    FogHalveSite {
        address: 0x1_4010_C7F5,
        original: [0xF3, 0x0F, 0x59, 0x05, 0x57, 0x1A, 0x1F, 0x02],
    },
];

// An 8-byte multi-byte NOP (`0F 1F 84 00 00 00 00 00`). Replacing each `mulss` with it leaves the
// full-resolution width/height in `xmm0`, so the following `cvttss2si` reads the full dimension.
const FOG_HALVE_NOP: [u8; 8] = [0x0F, 0x1F, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00];

// `CRenderBlockTypeFogVolume::ResizeTextures` -- recreates the froxel fog textures at a resolution
// change. While the fog full-res toggle is on, the two width/height halving multiplies are no-op'd
// around the call so the coarse volumetric-depth buffer is recreated at full resolution. Scoped: the
// patch is verified, applied, and reverted within this single call, which runs on the main thread with
// no draw in flight (the registered resolution-change callback), so no other code sees the patched
// bytes.
#[detour(address = RenderBlockTypeFogVolume::ResizeTextures_ADDRESS)]
fn fog_volume_resize_textures(this: *mut c_void, width: u32, height: u32) -> bool {
    let original = FOG_VOLUME_RESIZE_TEXTURES.get().unwrap();
    if !Config::lock_query(|c| c.stereo.fog_full_res) {
        return original.call(this, width, height);
    }
    // SAFETY: the halving sites are constant code addresses; `nop_fog_halving` verifies each still
    // holds the recorded `mulss` encoding before writing, and `restore_fog_halving` writes back the
    // recorded originals, all on the game thread within this call.
    let patched = unsafe { nop_fog_halving() };
    let result = original.call(this, width, height);
    if patched {
        unsafe { restore_fog_halving() };
    }
    result
}

/// No-op both fog halving `mulss` sites, but only if every site still holds its recorded encoding.
/// Returns whether the patch was applied (so the caller knows whether to restore).
///
/// # Safety
///
/// The recorded addresses must be valid, readable/writable code addresses (the game module's fixed
/// load base), and no other thread may execute or patch these sites during the call.
unsafe fn nop_fog_halving() -> bool {
    for site in &FOG_HALVE_SITES {
        let current = unsafe { std::slice::from_raw_parts(site.address as *const u8, 8) };
        if current != site.original {
            return false;
        }
    }
    for site in &FOG_HALVE_SITES {
        unsafe { write_code(site.address, &FOG_HALVE_NOP) };
    }
    true
}

/// Restore both fog halving `mulss` sites to their recorded original encoding.
///
/// # Safety
///
/// See [`nop_fog_halving`]; call only after it returned `true` in the same `ResizeTextures` call.
unsafe fn restore_fog_halving() {
    for site in &FOG_HALVE_SITES {
        unsafe { write_code(site.address, &site.original) };
    }
}

/// Write `bytes` over executable code at `address`, flipping the page to writable and back.
///
/// # Safety
///
/// `address` must be a valid code address with at least `bytes.len()` writable bytes.
unsafe fn write_code(address: usize, bytes: &[u8]) {
    let ptr = address as *mut u8;
    let mut old = PAGE_PROTECTION_FLAGS::default();
    if unsafe { VirtualProtect(ptr as _, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) }.is_err() {
        return;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    let _ = unsafe { VirtualProtect(ptr as _, bytes.len(), old, &mut old) };
}

/// The particle block type's low-res routing flags as they were before the full-res toggle first
/// forced them off, so the toggle turning off restores the engine's own (settings-driven) values.
/// `None` while the toggle is off / not yet applied.
static SAVED_LOW_RES_PARTICLES: Mutex<Option<(bool, bool)>> = Mutex::new(None);

// `CLRParticleRenderPass::Draw` -- runs every frame as part of the render-pass sequence, a convenient
// per-frame seam to reconcile the particle block type's low-res routing flags. The routing itself
// happens earlier (sim add-to-render), so clearing the flags here routes the *next* frame's particles
// to the full-resolution transparent pass; the pass draw is otherwise untouched.
#[detour(address = LRParticleRenderPass::Draw_ADDRESS)]
fn lr_particle_render_pass_draw(this: *mut LRParticleRenderPass) {
    reconcile_low_res_particles();
    LR_PARTICLE_RENDER_PASS_DRAW.get().unwrap().call(this);
}

/// Force the particle block type's low-res routing flags off while the full-res toggle is on (saving
/// the engine's values on the first apply), and restore them when it turns off.
fn reconcile_low_res_particles() {
    let full_res = Config::lock_query(|c| c.stereo.particles_full_res);
    // SAFETY: the particle block type is a process-lifetime singleton; the flags are plain bytes.
    let Some(block_type) = (unsafe { RenderBlockTypeParticle::get() }) else {
        return;
    };
    let mut saved = SAVED_LOW_RES_PARTICLES.lock();
    if full_res {
        if saved.is_none() {
            *saved = Some((
                block_type.m_LowResRendering,
                block_type.m_ForceLowResRendering,
            ));
        }
        // Re-forced every frame so an engine settings change cannot re-enable low-res mid-toggle.
        block_type.m_LowResRendering = false;
        block_type.m_ForceLowResRendering = false;
    } else if let Some((low_res, force_low_res)) = saved.take() {
        block_type.m_LowResRendering = low_res;
        block_type.m_ForceLowResRendering = force_low_res;
    }
}

// `CLightManager::CopyLightsToUpdate` -- the per-frame light gather that routes the volumetric
// spot-light cone through a quarter-resolution render setup when `enable_low_res_spot_light_volume` is
// set. While the spotlight full-res toggle is on, the global is scoped to `false` around the gather so
// the engine's own full-resolution branch runs (main render setup, cone block type low-res flag
// cleared); the saved value is restored after, so the engine's setting is untouched when off.
#[detour(address = LightManager::CopyLightsToUpdate_ADDRESS)]
fn copy_lights_to_update(this: *mut LightManager, reserved_slots: u32, dt: f32) {
    let original = COPY_LIGHTS_TO_UPDATE.get().unwrap();
    if !Config::lock_query(|c| c.stereo.spotlight_full_res) {
        original.call(this, reserved_slots, dt);
        return;
    }
    // SAFETY: the global is a process-lifetime engine bool; the scoped write runs on the game thread
    // for the duration of the (single-threaded) gather.
    let flag = unsafe { get_enable_low_res_spot_light_volume() };
    let saved = *flag;
    *flag = false;
    original.call(this, reserved_slots, dt);
    *flag = saved;
}
