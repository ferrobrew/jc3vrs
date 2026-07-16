//! Diagnostic detour on `CRenderBlockTerrain::HullClipType` to override the base VolumetricTerrain
//! color-pass hull-clip type.
//!
//! `HullClipType` returns the clip type (0, 1, or 2) that `CRenderBlockTerrain::Draw` uses to index the
//! hull program. The color pass resolves to type 2 -- the LOD-clipping hull, which discards tessellated
//! patches by their LOD against the tessellation metrics -- while the depth prepass uses a non-clipping
//! variant. A patch discarded by that color-pass hull writes depth (from the prepass) but no G-buffer,
//! so the deferred lighting has nothing to light there and the tile renders black; in VR's wide FOV the
//! discard drops grazing wall/ceiling patches the eye can actually see.
//!
//! This detour, gated by [`StereoConfig::force_terrain_hull_clip`](crate::config::StereoConfig),
//! replaces a returned clip type 2 with the configured
//! [`terrain_hull_clip_value`](crate::config::StereoConfig::terrain_hull_clip_value), so the color pass
//! uses a non-clipping hull. If the black tiles then fill into the G-buffer, the color-pass LOD clip is
//! the source.

use detours_macro::detour;
use jc3gi::graphics_engine::{graphics_engine::RenderContext, render_block::RenderBlockTerrain};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&HULL_CLIP_TYPE_BINDER)
}

#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockTerrain::HullClipType_ADDRESS)]
fn hull_clip_type(this: *const RenderBlockTerrain, render_context: *mut RenderContext) -> i64 {
    let original = HULL_CLIP_TYPE.get().unwrap().call(this, render_context);
    let (force, value) = Config::lock_query(|c| {
        (
            c.stereo.force_terrain_hull_clip,
            c.stereo.terrain_hull_clip_value,
        )
    });
    // Only the LOD-clipping color-pass result (type 2) is overridden, so the depth prepass and near
    // passes keep their own clip types.
    if force && original == 2 {
        i64::from(value)
    } else {
        original
    }
}
