//! UI manager detours: world-to-screen compensation for the floating HUD panel.
//!
//! [`get_2d_info`](UIManager::Get2DInfo) is the gameplay marker placement function. When the HUD
//! is redirected and drawn as a floating quad, the VP and camera matrix are replaced with the
//! panel's orientation so that markers project onto the panel's surface rather than the screen
//! plane. See `docs/hud.md`.

use detours_macro::detour;
use jc3gi::{
    types::math::{Matrix4, Vector2, Vector3},
    ui::ui_manager::{ScreenPos, UIManager},
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_2D_INFO_BINDER)
}

/// Replace the VP and camera matrix in `Get2DInfo` with the floating panel's orientation, so
/// world-to-screen projects markers onto the panel surface rather than the screen plane. When the
/// HUD redirect or quad is off, the original caller's VP is passed through unchanged.
//
// The parameter count matches the game's `CUIManager::Get2DInfo` ABI — the detour macro requires an
// exact signature match, so the parameters cannot be bundled into a struct.
#[detour(address = UIManager::Get2DInfo_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn get_2d_info(
    this: *mut UIManager,
    world: *const Vector3,
    vp_orig: *const Matrix4,
    camera_orig: *const Matrix4,
    a5: f32,
    out_x: *mut f32,
    out_y: *mut f32,
    out_pos: *mut ScreenPos,
    margin: f32,
    a10: bool,
    offset: Vector2,
) {
    let (panel_enabled, aspect) =
        Config::lock_query(|c| (c.hud.redirect && c.hud.quad, c.hud.aspect));
    let panel = panel_enabled.then(crate::hud::compute_panel_vp).flatten();
    let (vp, camera) = panel
        .as_ref()
        .map(|(v, c)| (v as *const Matrix4, c as *const Matrix4))
        .unwrap_or((vp_orig, camera_orig));

    // For the panel pass, retarget `Convert3DCoords`' aspect correction to the panel: it reads
    // `m_CachedViewportRatio` (the window height/width, refreshed every frame), which would skew
    // markers off the panel on top of the already-re-aspected panel VP. Set it to the panel's
    // `height / width` (= `1 / aspect`) for the call and restore it afterwards so other
    // world-to-screen consumers in the same frame are unaffected.
    let restore_ratio = panel
        .is_some()
        .then(|| unsafe {
            this.as_mut().map(|manager| {
                let previous = manager.m_CachedViewportRatio;
                manager.m_CachedViewportRatio = 1.0 / aspect.max(f32::EPSILON);
                previous
            })
        })
        .flatten();

    GET_2D_INFO.get().unwrap().call(
        this, world, vp, camera, a5, out_x, out_y, out_pos, margin, a10, offset,
    );

    if let Some(previous) = restore_ratio {
        unsafe {
            if let Some(manager) = this.as_mut() {
                manager.m_CachedViewportRatio = previous;
            }
        }
    }
}
