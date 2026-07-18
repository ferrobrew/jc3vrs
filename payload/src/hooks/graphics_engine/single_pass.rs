//! Render-thread detours for single-pass stereo (experimental; see `docs/mod/single-pass-stereo.md`
//! and [`crate::stereo::single_pass`]).
//!
//! When single-pass is active, the patched vertex shaders read their position from the mod-owned
//! `cb13` instead of `cb0`. This detour keeps `cb13` in sync: after the engine refreshes and uploads
//! the global VS constants (`m_VPGlobalConstData` → `cb0`), it mirrors the current view's per-eye
//! rows into `cb13` and binds it at `b13`. It runs per pass (the same cadence as the engine's own
//! `cb0` upload), so `cb13` always matches whatever view is current -- the G-buffer eye view, but
//! also the shadow/reflection views that reuse the same model shaders.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::render_engine::RenderEngine;
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS_BINDER)
}

#[detour(
    address = jc3gi::graphics_engine::render_engine::RenderEngine::SetAllGlobalShaderProgramConstants_ADDRESS
)]
fn set_all_global_shader_program_constants(this: *mut RenderEngine, ctx: *const c_void) {
    SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(this, ctx);
    if crate::stereo::single_pass::active()
        && let Some(engine) = unsafe { this.as_ref() }
    {
        crate::stereo::single_pass::mirror_and_bind_cb13(engine);
    }
}
