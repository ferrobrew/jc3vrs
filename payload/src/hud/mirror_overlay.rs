//! The flat debug-UI overlay for the desktop mirror, rendered into an offscreen texture.
//!
//! Normally the flat egui overlay is drawn straight onto the mirror's back buffer by
//! [`EguiState::render`](crate::egui_impl::EguiState::render). That call mutates the shared
//! `EguiState` and issues D3D work, so it is only safe on the game thread after the frame's egui
//! pass — which the deferred frame tail ([`crate::vr::tail`]) is not. So when the tail is deferred,
//! render the overlay once on the draw thread (eye 0's post-draw, where the panel renders too), into
//! an offscreen texture, and let the mirror composite that texture from whichever thread runs it —
//! exactly the model the floating panel (issue #24) already uses.
//!
//! Only active while a VR session renders, the mirror is on, and the floating panel is off (with the
//! panel on, the mirror composites the panel texture instead). Off, every path is the untouched flat
//! overlay.

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11ShaderResourceView};

use super::target::HudTarget;

/// The overlay texture, sized to the back buffer, holding this frame's egui output.
static OVERLAY: Mutex<Option<HudTarget>> = Mutex::new(None);

/// Whether the flat overlay should be redirected into the offscreen texture this frame: a VR frame is
/// rendering, the mirror is on, and the floating panel is off. Read on the draw thread (the render
/// gate) and the mirror thread (the composite gate), so it uses the draw-safe render-params signal,
/// not the VR runtime lock (see [`crate::hud::egui_panel::is_active`] for why).
pub(crate) fn is_active() -> bool {
    crate::vr::render_params(0).is_some()
        && crate::config::Config::lock_query(|c| c.vr.mirror && !c.hud.egui_panel.enabled)
}

/// Render this frame's egui output into the overlay texture. Called from the eye-0 post-draw hook
/// with the engine context mutex held, after the floating panel (a no-op while the panel is on, so
/// only one of the two consumes the single-use egui output). Sizes the texture to `target` (the eye
/// back buffer), so the mirror composites it 1:1.
pub(crate) fn render(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    if !is_active() {
        // Drop the texture when inactive so a later activation rebuilds at the current size.
        *OVERLAY.lock() = None;
        return;
    }
    let (width, height) = (
        u32::from(target.m_Width).max(1),
        u32::from(target.m_Height).max(1),
    );
    let mut overlay = OVERLAY.lock();
    if overlay.as_ref().map(HudTarget::size) != Some((width, height)) {
        match HudTarget::new(device, width, height) {
            Ok(target) => *overlay = Some(target),
            Err(e) => {
                tracing::error!("mirror overlay target: {e:#}");
                *overlay = None;
            }
        }
    }
    let Some(overlay) = overlay.as_ref() else {
        return;
    };
    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.render_to(context, overlay.color_rtv());
    }
}

/// A clone of the overlay texture's shader-resource view for the desktop mirror composite, or `None`
/// when the overlay is inactive or not yet built. Locks [`OVERLAY`] briefly.
pub(crate) fn overlay_srv() -> Option<ID3D11ShaderResourceView> {
    OVERLAY.lock().as_ref().map(|t| t.color_srv().clone())
}
