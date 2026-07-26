//! The VR tab: OpenXR runtime status, a Recenter button, and the live-editable runtime toggles.

use crate::{config, headpose, vr, vr::MirrorFraming};

pub fn egui_debug_vr(ui: &mut egui::Ui) {
    let status = vr::status();

    ui.label(format!("Session: {}", session_label(&status)));
    ui.label(match &status.runtime_name {
        Some(name) => format!("Runtime: {name}"),
        None => "Runtime: (none)".to_string(),
    });
    ui.label(match status.eye_resolution {
        Some((w, h)) => format!("Per-eye resolution: {w} × {h}"),
        None => "Per-eye resolution: (no session)".to_string(),
    });
    ui.label(format!("Headpose source: {:?}", headpose::source()));

    if ui
        .button("Recenter")
        .on_hover_text("Re-base the cockpit baseline to the current head pose (also bound to F7).")
        .clicked()
    {
        headpose::recenter();
    }

    ui.separator();

    // Live-editable, mutating the shared config directly (the frame loop reads it each frame), the
    // same pattern the other tabs use.
    let mut cfg = config::CONFIG.lock();
    ui.checkbox(&mut cfg.vr.enabled, "Enabled (bring up the OpenXR session)")
        .on_hover_text("Off leaves the mod in flatscreen stereo and tears any live runtime down.");
    ui.checkbox(&mut cfg.vr.native_resolution, "Native per-eye resolution")
        .on_hover_text(
            "Drive the engine to render each eye at the HMD-recommended resolution; disabled \
             automatically on a resize fault.",
        );
    ui.add_enabled_ui(cfg.vr.native_resolution, |ui| {
        ui.indent("resolution", |ui| {
            ui.add(
                egui::Slider::new(&mut cfg.vr.resolution_scale, 0.25..=2.0)
                    .text("Per-eye resolution scale"),
            )
            .on_hover_text(
                "Multiplies the runtime's recommended per-eye size to give the engine's render \
                 resolution. Takes effect on the next frame -- the driver notices the target size \
                 changed and issues a resize. The OpenXR swapchain keeps the size it was created \
                 at, so a lower scale renders smaller and the blit upscales into it; that is what \
                 makes this an honest A/B for whether GPU cost tracks pixel count.",
            );
        });
    });
    ui.checkbox(&mut cfg.vr.mirror, "Desktop mirror")
        .on_hover_text("Show one eye in the game window while a session runs.");
    ui.add_enabled_ui(cfg.vr.mirror, |ui| {
        ui.indent("mirror", |ui| {
            ui.horizontal(|ui| {
                ui.label("Eye");
                ui.selectable_value(&mut cfg.vr.mirror_eye, 0, "Left");
                ui.selectable_value(&mut cfg.vr.mirror_eye, 1, "Right");
            });
            ui.horizontal(|ui| {
                ui.label("Framing").on_hover_text(
                    "How the near-square eye image is reconciled with the widescreen window.",
                );
                ui.selectable_value(&mut cfg.vr.mirror_framing, MirrorFraming::Fill, "Fill")
                    .on_hover_text(
                        "Crop to the window and fill it edge to edge, like other VR titles' desktop \
                         views.",
                    );
                ui.selectable_value(&mut cfg.vr.mirror_framing, MirrorFraming::Fit, "Fit")
                    .on_hover_text(
                        "Letterbox the whole eye render, showing everything the eye drew including \
                         the edges.",
                    );
            });
            ui.add(
                egui::Slider::new(&mut cfg.vr.mirror_zoom, vr::MIRROR_ZOOM_RANGE)
                    .text("Zoom"),
            )
                .on_hover_text(
                    "Magnify about the centre on top of the framing. Above 1.0 crops in further, \
                     tightening the desktop view onto the middle of the eye's much wider field of \
                     view.",
                );
        });
    });
    ui.checkbox(&mut cfg.body_ik.enabled, "Body IK")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
    ui.checkbox(&mut cfg.vr.own_back_buffer, "Mod-owned back buffer")
        .on_hover_text(
            "Render into a mod-owned target so the DXGI swapchain stays at the window size instead \
             of following the per-eye render resolution. Makes the desktop mirror's present 1:1 and \
             stops single-pass double-wide forcing a 2x-width swapchain. Takes effect on the next \
             resize, which toggling this issues.",
        );
}

/// A human-readable label for the runtime's current session state.
fn session_label(status: &vr::VrStatus) -> &'static str {
    if status.busy {
        // The runtime lock was held by the frame tail; the rest of the snapshot is not meaningful.
        return "(busy -- the frame tail holds the runtime lock)";
    }
    if !status.enabled {
        "disabled (flatscreen stereo)"
    } else if status.running {
        "running"
    } else if status.instance_up {
        "instance up, session idle"
    } else {
        "no runtime (retrying)"
    }
}
