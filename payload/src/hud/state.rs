//! The HUD-redirect state machine: it lazily creates the target, applies and relinquishes the rebind,
//! and owns the egui preview registration.

use jc3gi::graphics_engine::device::Device;
use parking_lot::Mutex;

use super::{binding, target::HudTarget};

/// Global HUD state. Locked briefly on the render thread.
pub static HUD_STATE: Mutex<HudState> = Mutex::new(HudState::new());

/// The live HUD-redirect state (render thread only, apart from the preview registration on the UI
/// thread).
pub struct HudState {
    target: Option<HudTarget>,
    /// Whether the redirect is currently applied to the UI's render buffer.
    redirected: bool,
    /// The egui texture id for the HUD preview, registered lazily on the UI thread.
    preview_id: Option<egui::TextureId>,
}

impl HudState {
    const fn new() -> Self {
        Self {
            target: None,
            redirected: false,
            preview_id: None,
        }
    }

    /// Ensure the HUD is redirected into our target at `width` x `height`, (re)creating the target on a
    /// size change and applying the rebind once. A failed target build or a not-yet-live UI leaves the
    /// state unredirected, so the next tick retries.
    pub(super) fn ensure_redirected(&mut self, device: &Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        if self.target.as_ref().map(HudTarget::size) != Some((width, height)) {
            match HudTarget::new(device, width, height) {
                Ok(target) => {
                    self.target = Some(target);
                    self.redirected = false;
                    // The preview SRV belonged to the old texture; re-register it on the next preview.
                    self.preview_id = None;
                }
                Err(e) => {
                    tracing::error!("HUD: {e:#}");
                    self.target = None;
                    return;
                }
            }
        }

        let Some(target) = self.target.as_ref() else {
            return;
        };
        if !self.redirected && binding::redirect_to(target) {
            self.redirected = true;
        }
    }

    /// Restore the engine's own binding and drop our target, so the UI no longer renders into our
    /// texture. A no-op when not redirected. `width` is the back-buffer width `InitPlatformRT` expects.
    pub(super) fn restore(&mut self, width: u32) {
        if self.redirected {
            binding::restore_engine_binding(width);
            self.redirected = false;
        }
        self.target = None;
    }

    /// Register (once) and return the egui texture id for previewing the redirected HUD. `None` until
    /// the HUD has been redirected into our texture.
    pub fn preview_id(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
    ) -> Option<egui::TextureId> {
        if self.preview_id.is_none()
            && let Some(target) = self.target.as_ref()
        {
            self.preview_id = Some(renderer.register_user_texture(target.color_srv().clone()));
        }
        self.preview_id
    }

    /// Drop the egui preview registration. Call on the UI thread (it owns the renderer), so the texture
    /// id is released rather than leaked.
    pub fn release_preview(&mut self, renderer: &mut egui_directx11::Renderer) {
        if let Some(id) = self.preview_id.take() {
            renderer.unregister_user_texture(id);
        }
    }
}
