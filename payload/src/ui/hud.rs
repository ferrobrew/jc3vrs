//! egui debug UI: the HUD tab. Controls the HUD redirect, the floating-quad placement and follow
//! parameters, and a preview of the redirected HUD texture.

use parking_lot::Mutex;

use crate::config;

/// Preview size (px) for the redirected HUD texture, independent of the Render tab's preview size.
static HUD_PREVIEW_WIDTH: Mutex<f32> = Mutex::new(512.0);

/// The clip path the Scaleform visibility controls operate on. Editable so paths from a tree dump
/// can be tried live without a rebuild.
static SCALEFORM_CLIP_PATH: Mutex<String> = Mutex::new(String::new());

pub fn egui_debug_hud(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    // Live dynamic-distance readout, taken before the CONFIG lock (HUD_STATE and CONFIG are
    // never nested in the draw path, but keeping them disjoint here avoids the question).
    let depth_status = crate::hud::HUD_STATE.lock().depth_status();

    // Redirect toggle and the quad placement/follow parameters. The CONFIG lock is scoped to this
    // block and dropped before HUD_STATE is locked for the preview.
    let redirect = {
        let mut cfg = config::CONFIG.lock();
        ui.checkbox(
            &mut cfg.hud.redirect,
            "Redirect HUD into our texture (drops it from the scene composite)",
        );
        ui.add_enabled_ui(cfg.hud.redirect, |ui| {
            ui.add(egui::Slider::new(&mut cfg.hud.hud_aspect, 0.5..=2.5).text("HUD aspect (w/h)"));
            ui.add(
                egui::Slider::new(&mut cfg.hud.movie_aspect, 0.5..=2.5).text("Movie aspect (w/h)"),
            );
            ui.add(
                egui::Slider::new(&mut cfg.hud.render_scale, 0.1..=2.0).text("Render scale (x)"),
            );
            ui.checkbox(&mut cfg.hud.quad, "Draw the HUD as a floating quad per eye");
            ui.checkbox(
                &mut cfg.hud.suppress_overlays,
                "Suppress full-screen overlays (damage flash, drowning)",
            );
            ui.checkbox(
                &mut cfg.hud.world_lock_menus,
                "World-lock panel in menus (pause/map stay put instead of head-following)",
            );
            ui.checkbox(
                &mut cfg.hud.cursor.enabled,
                "Virtual mouse cursor on the panel (remaps mouse-to-UI coordinates)",
            );
            if cfg.hud.cursor.enabled {
                ui.indent("hud_cursor", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.cursor.size, 0.004..=0.05)
                            .text("Cursor size (fraction of distance)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.cursor.lift, 0.0..=0.2)
                            .text("Cursor lift off the panel (m)"),
                    );
                });
            }
            ui.checkbox(
                &mut cfg.hud.marker_warp,
                "Warp the panel to per-element world depths (markers + center bubble)",
            );
            if cfg.hud.marker_warp {
                ui.indent("hud_warp", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_radius, 0.01..=0.3)
                            .text("Marker warp radius (uv)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_max_depth, 10.0..=1000.0)
                            .logarithmic(true)
                            .text("Marker depth clamp (m)"),
                    );
                    ui.checkbox(
                        &mut cfg.hud.center_depth_from_aim,
                        "Drive the center bubble's depth from the aim point",
                    );
                    if cfg.hud.center_depth_from_aim {
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.center_bubble_radius, 0.01..=0.4)
                                .text("Center bubble radius (uv)"),
                        );
                    }
                });
            }
            ui.checkbox(
                &mut cfg.hud.split,
                "PARKED: split the HUD into depth layers (breaks the UI on pause)",
            );
            if cfg.hud.split {
                ui.indent("hud_split", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.marker_distance, 0.3..=50.0)
                            .logarithmic(true)
                            .text("Marker layer distance (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.center_distance, 0.3..=10.0)
                            .text("Center layer distance (m)"),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Clip path prefix");
                        let mut prefix = cfg.hud.split_path_prefix.as_str().to_string();
                        if ui.text_edit_singleline(&mut prefix).changed()
                            && let Err(e) = cfg.hud.split_path_prefix.set(&prefix)
                        {
                            tracing::warn!("{e}");
                        }
                    });
                });
            }
            ui.checkbox(
                &mut cfg.hud.depth_shift.enabled,
                "Dynamic distance from the scene depth distribution",
            );
            if cfg.hud.depth_shift.enabled {
                ui.indent("hud_depth_shift", |ui| {
                    match depth_status {
                        Some(status) => {
                            let state = if status.near_engaged { "near" } else { "base" };
                            let distance = status
                                .smoothed
                                .map_or("-".to_string(), |d| format!("{d:.2} m"));
                            match status.stats {
                                // The histogram statistics only exist while that policy runs.
                                Some(stats) if cfg.hud.depth_shift.use_depth_histogram => {
                                    ui.label(format!(
                                        "Live: {state} at {distance} (near {:.0}%, p{:.2} m)",
                                        stats.near_occupancy * 100.0,
                                        stats.percentile_depth,
                                    ));
                                }
                                _ if cfg.hud.depth_shift.use_depth_histogram => {
                                    ui.label(format!(
                                        "Live: {state} at {distance} (no depth samples yet)"
                                    ));
                                }
                                _ => {
                                    ui.label(format!(
                                        "Live: {state} at {distance} ({})",
                                        if status.near_engaged {
                                            "in a vehicle"
                                        } else {
                                            "not in a vehicle"
                                        },
                                    ));
                                }
                            }
                        }
                        None => {
                            ui.label("Live: not sampled yet");
                        }
                    }
                    ui.checkbox(
                        &mut cfg.hud.depth_shift.use_depth_histogram,
                        "EXPERIMENTAL: drive from the depth histogram instead of the vehicle \
                         state",
                    );
                    if cfg.hud.depth_shift.use_depth_histogram {
                        ui.checkbox(
                            &mut cfg.hud.depth_shift.mask_by_hud,
                            "Weight samples by the HUD's alpha on the panel",
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.min_depth, 0.0..=2.0)
                                .text("Ignore depths below (m)"),
                        );
                    }
                    if cfg.hud.depth_shift.use_depth_histogram && cfg.hud.depth_shift.continuous {
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.percentile, 0.01..=0.5)
                                .text("Percentile"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.margin, 0.0..=2.0)
                                .text("Margin inside (m)"),
                        );
                    } else if cfg.hud.depth_shift.use_depth_histogram {
                        ui.checkbox(
                            &mut cfg.hud.depth_shift.continuous,
                            "EXPERIMENTAL: follow the depth percentile continuously",
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.near_threshold, 0.3..=10.0)
                                .text("Near-field threshold (m)"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.near_occupancy, 0.01..=0.9)
                                .text("Near occupancy to engage"),
                        );
                        ui.add(
                            egui::Slider::new(&mut cfg.hud.depth_shift.hysteresis, 0.0..=0.3)
                                .text("Release hysteresis"),
                        );
                    }
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.depth_shift.near_distance, 0.3..=3.0)
                            .text("Near distance (m)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.depth_shift.halflife, 0.05..=2.0)
                            .text("Easing halflife (s)"),
                    );
                });
            }
            if cfg.hud.quad {
                ui.indent("hud_sliders", |ui| {
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.distance, 0.3..=10.0)
                            .text("Base distance (m)"),
                    );
                    ui.add(egui::Slider::new(&mut cfg.hud.panel_scale, 0.2..=3.0).text("Size (x)"));
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.rotation_halflife, 0.01..=2.0)
                            .text("Rotation halflife (s)"),
                    );
                    ui.add(
                        egui::Slider::new(&mut cfg.hud.follow.position_halflife, 0.01..=1.0)
                            .text("Position halflife (s)"),
                    );
                });
            }
        });
        cfg.hud.redirect
    };

    if redirect {
        // Preview matches the current mode's effective aspect, so it tracks the live texture shape.
        let aspect = crate::hud::current_aspect();
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
        let split_enabled = crate::config::Config::lock_query(|c| c.hud.split);
        if split_enabled {
            egui::CollapsingHeader::new("Split layer textures")
                .default_open(false)
                .show(ui, |ui| {
                    let ids = hud.layer_preview_ids(renderer);
                    let size = egui::vec2(preview_width, preview_width / aspect.max(f32::EPSILON));
                    for (id, label) in ids.iter().zip(["Markers", "Center"]) {
                        ui.label(label);
                        match id {
                            Some(id) => {
                                ui.add(egui::Image::new(egui::ImageSource::Texture(
                                    egui::load::SizedTexture { id: *id, size },
                                )));
                            }
                            None => {
                                ui.label("(layer target not created)");
                            }
                        }
                    }
                });
        }
    }

    scaleform_debug_ui(ui);
}

/// The Scaleform display-tree debug controls: dump the live clip tree to the log, and toggle a
/// clip's `_visible` by path. Requests are queued here and executed on the game thread.
fn scaleform_debug_ui(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Scaleform display tree")
        .default_open(false)
        .show(ui, |ui| {
            if ui
                .button("Auto-configure split from display tree")
                .on_hover_text(
                    "Finds the HUD clip in the live tree, sets the split path prefix, and \
                     collects the anonymous POI pool for the markers layer.",
                )
                .clicked()
            {
                crate::hud::scaleform::request_layout_discovery();
            }
            if ui
                .button("Dump display tree to log")
                .on_hover_text(
                    "Walks the live movie's clip tree on the game thread and logs one line per \
                     clip, as dot-joined paths.",
                )
                .clicked()
            {
                crate::hud::scaleform::request_dump_tree();
            }
            ui.horizontal(|ui| {
                let mut path = SCALEFORM_CLIP_PATH.lock();
                if path.is_empty() {
                    *path = "MCI_poi_stage".to_string();
                }
                ui.label("Clip path");
                ui.text_edit_singleline(&mut *path);
                if ui.button("Hide").clicked() {
                    crate::hud::scaleform::request_set_clip_visible(path.clone(), false);
                }
                if ui.button("Show").clicked() {
                    crate::hud::scaleform::request_set_clip_visible(path.clone(), true);
                }
            });
        });
}
