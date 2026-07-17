//! The Previews tab: the per-eye pipeline thumbnails, the fusable stereo pair, and the live
//! render-target views, all fed by the capture state in [`super::render`].

use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use windows::core::Interface;

use super::render::{EGUI_DEBUG_RENDER_STATE, EguiDebugRenderState, STEREO_CROSS_EYED};

/// Labels for the post-effect stages captured per eye, in chain order (indices
/// [`super::render::POST_STAGE_DOF`] and [`super::render::POST_STAGE_MB`]).
const POST_STAGE_LABELS: [&str; 2] = ["after DoF", "after MB"];

/// Preview thumbnail width (px); user-controllable via a slider.
static PREVIEW_WIDTH: Mutex<f32> = Mutex::new(700.0);

pub fn egui_debug_previews(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    let preview_width = {
        let mut w = PREVIEW_WIDTH.lock();
        ui.add(egui::Slider::new(&mut *w, 48.0..=4096.0).text("Preview size (px)"));
        *w
    };

    // The capture textures are prepared every frame by `egui_debug_window` (the VR blit depends
    // on them), so this tab only reads them.
    let mut state = EGUI_DEBUG_RENDER_STATE.lock();

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Per-eye pipeline rows: each row is one eye, columns are pipeline stages.
            let aspect = unsafe {
                jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()
                    .and_then(|ge| ge.m_MainColorBuffer.as_mut())
                    .map(|mcb| mcb.m_Height as f32 / (mcb.m_Width.max(1) as f32))
            }
            .unwrap_or(0.5625);
            let size = egui::vec2(preview_width, preview_width * aspect);
            // Register any post-stage SRVs that were created on the render thread.
            for cap in &mut state.post_stage_captures {
                if cap.egui_id.is_none()
                    && let Some(srv) = &cap.srv
                {
                    cap.egui_id = Some(renderer.register_user_texture(srv.clone()));
                }
            }

            // Build the columns in pipeline order: Scene -> after DoF -> after MB -> Final.
            let columns: Vec<(&str, [Option<egui::TextureId>; 2])> = {
                let post_id = |stage: usize, eye: usize| -> Option<egui::TextureId> {
                    state
                        .post_stage_captures
                        .get(stage * 2 + eye)
                        .and_then(|c| c.egui_id)
                };
                let mc_id = |eye: usize| state.main_color_textures[eye].as_ref().map(|(_, id)| *id);
                let bb_id = |eye: usize| state.target_textures[eye].as_ref().map(|(_, id)| *id);
                vec![
                    ("Scene", [mc_id(0), mc_id(1)]),
                    (POST_STAGE_LABELS[0], [post_id(0, 0), post_id(0, 1)]),
                    (POST_STAGE_LABELS[1], [post_id(1, 0), post_id(1, 1)]),
                    ("Final", [bb_id(0), bb_id(1)]),
                ]
            };

            // Stereo pair: the two eyes' final images side by side, for fusing into one 3D image.
            // Parallel (default): left image -> left eye. Cross-eyed: tick the box to swap them.
            let final_ids = columns
                .iter()
                .find(|(name, _)| *name == "Final")
                .map(|(_, ids)| *ids)
                .unwrap_or([None, None]);
            egui::CollapsingHeader::new("Stereo pair (side-by-side — fuse for 3D)")
                .default_open(true)
                .show(ui, |ui| {
                    gate_checkbox(
                        ui,
                        &STEREO_CROSS_EYED,
                        "Cross-eyed (swap L/R; off = parallel/wall-eyed)",
                    );
                    let order = if STEREO_CROSS_EYED.load(Ordering::Relaxed) {
                        [1usize, 0]
                    } else {
                        [0usize, 1]
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for eye in order {
                            match final_ids[eye] {
                                Some(id) => {
                                    ui.add(egui::Image::new(egui::ImageSource::Texture(
                                        egui::load::SizedTexture { id, size },
                                    )));
                                }
                                None => {
                                    ui.add_sized(size, egui::Label::new("(no capture)"));
                                }
                            }
                        }
                    });
                });

            ui.collapsing("Per-eye pipeline (rows = eyes, columns = stages)", |ui| {
                for eye in 0..2 {
                    ui.label(format!("Eye {eye}"));
                    ui.horizontal(|ui| {
                        for (name, ids) in &columns {
                            ui.vertical(|ui| {
                                match ids[eye] {
                                    Some(id) => {
                                        ui.add(egui::Image::new(egui::ImageSource::Texture(
                                            egui::load::SizedTexture { id, size },
                                        )));
                                    }
                                    None => {
                                        ui.add_sized(size, egui::Label::new("(no capture)"));
                                    }
                                }
                                if eye == 1 {
                                    ui.label(*name);
                                }
                            });
                        }
                    });
                }
            });

            ui.collapsing("Render targets (live; eye 1 is the last-rendered pass)", |ui| {
                let targets: Vec<(&str, *mut jc3gi::graphics_engine::texture::Texture)> = unsafe {
                    match jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() {
                        Some(ge) => vec![
                            ("MainColor", ge.m_MainColorBuffer),
                            ("MainDepth", ge.m_MainDepthTexture),
                            ("DownsampledDepth", ge.m_DownSampledDepthTexture),
                            ("GBuffer0", ge.m_GBufferTexture[0]),
                            ("GBuffer1", ge.m_GBufferTexture[1]),
                            ("GBuffer2", ge.m_GBufferTexture[2]),
                            ("GBuffer3", ge.m_GBufferTexture[3]),
                            ("Velocity", ge.m_VelocityBufferTexture),
                            ("BackBufferLinear", ge.m_BackBufferLinear),
                        ],
                        None => vec![],
                    }
                };
                ui.horizontal_wrapped(|ui| {
                    for (label, texture) in targets {
                        show_target_thumbnail(ui, &mut state, renderer, label, texture, preview_width);
                    }
                });
            });
        });
}

fn gate_checkbox(ui: &mut egui::Ui, flag: &std::sync::atomic::AtomicBool, label: &str) {
    let mut v = flag.load(Ordering::Relaxed);
    if ui.checkbox(&mut v, label).changed() {
        flag.store(v, Ordering::Relaxed);
    }
}

fn show_target_thumbnail(
    ui: &mut egui::Ui,
    state: &mut EguiDebugRenderState,
    renderer: &mut egui_directx11::Renderer,
    label: &str,
    texture: *mut jc3gi::graphics_engine::texture::Texture,
    width: f32,
) {
    unsafe {
        let Some(tex) = texture.as_ref() else {
            ui.label(format!("{label}: null"));
            return;
        };
        let size = egui::vec2(
            width,
            width * tex.m_Height as f32 / (tex.m_Width.max(1) as f32),
        );
        let srv_raw = tex.m_SRV.as_raw() as usize;
        ui.vertical(|ui| {
            if srv_raw == 0 {
                ui.label(format!("{label}: no SRV"));
            } else {
                let id = state.thumbnail_id(renderer, srv_raw, &tex.m_SRV);
                ui.add(egui::Image::new(egui::ImageSource::Texture(
                    egui::load::SizedTexture { id, size },
                )));
            }
            ui.label(format!(
                "{} {}x{} f{}",
                label, tex.m_Width, tex.m_Height, tex.m_Format
            ));
        });
    }
}
