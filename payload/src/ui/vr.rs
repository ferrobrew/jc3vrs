//! The VR tab: OpenXR runtime status, a Recenter button, and the live-editable runtime toggles.

use crate::{config, headpose, vr};

pub fn egui_debug_vr(ui: &mut egui::Ui) {
    let status = vr::status();

    ui.heading("OpenXR runtime");
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
    ui.checkbox(&mut cfg.vr.mirror, "Desktop mirror")
        .on_hover_text("Show one eye in the game window while a session runs.");
    ui.checkbox(&mut cfg.body_ik.enabled, "Body IK")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
}

/// A human-readable label for the runtime's current session state.
fn session_label(status: &vr::VrStatus) -> &'static str {
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
