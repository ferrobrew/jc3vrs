//! egui debug UI: the HUD tab. Controls the HUD redirect, the floating-quad placement and follow
//! parameters, and a preview of the redirected HUD texture.

use parking_lot::Mutex;

use crate::config;

/// Preview size (px) for the redirected HUD texture, independent of the Render tab's preview size.
static HUD_PREVIEW_WIDTH: Mutex<f32> = Mutex::new(512.0);

pub fn egui_debug_hud(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    // Redirect toggle and the quad placement/follow parameters. The CONFIG lock is scoped to this
    // block and dropped before HUD_STATE is locked for the preview.
    let (redirect, aspect) = {
        let mut cfg = config::CONFIG.lock();
        ui.checkbox(
            &mut cfg.hud.redirect,
            "Redirect HUD into our texture (drops it from the scene composite)",
        );
        ui.add_enabled_ui(cfg.hud.redirect, |ui| {
            ui.add(egui::Slider::new(&mut cfg.hud.aspect, 0.5..=2.5).text("Aspect (w/h)"));
            ui.add(
                egui::Slider::new(&mut cfg.hud.render_scale, 0.1..=2.0).text("Render scale (x)"),
            );
            ui.checkbox(&mut cfg.hud.quad, "Draw the HUD as a floating quad per eye");
            ui.add_enabled_ui(cfg.hud.quad, |ui| {
                ui.indent("hud_sliders", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.distance, 0.3..=10.0).text("Distance (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.panel_height, 0.2..=5.0)
                            .text("Panel height (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.rotation_halflife, 0.01..=2.0)
                            .text("Rotation halflife (s)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.position_halflife, 0.01..=1.0)
                            .text("Position halflife (s)"),
                    );
                });
            });
        });
        (cfg.hud.redirect, cfg.hud.aspect)
    };

    if redirect {
        let preview_width = {
            let mut w = HUD_PREVIEW_WIDTH.lock();
            ui.add(egui::Slider::new(&mut *w, 48.0..=4096.0).text("Preview size (px)"));
            *w
        };
        let mut hud = crate::hud::HUD_STATE.lock();
        egui::CollapsingHeader::new("HUD texture")
            .default_open(false)
            .show(ui, |ui| match hud.preview_id(renderer) {
                Some(id) => {
                    // The preview matches the HUD aspect (width / height).
                    let size = egui::vec2(preview_width, preview_width / aspect.max(f32::EPSILON));
                    ui.add(egui::Image::new(egui::ImageSource::Texture(
                        egui::load::SizedTexture { id, size },
                    )));
                }
                None => {
                    ui.label("(redirect not yet applied)");
                }
            });
    }
}
