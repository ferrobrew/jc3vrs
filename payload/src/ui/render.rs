//! The Render tab: how the frame is produced — the core stereo widgets, FSR, and the stereo
//! correctness/quality levers — plus the render-thread capture state that feeds the Previews tab
//! and the VR blit.

use parking_lot::Mutex;
use windows::Win32::{
    Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
    },
    System::Threading::{EnterCriticalSection, LeaveCriticalSection},
};

use crate::{config, hooks::graphics_engine::shader, stereo::single_pass};

/// Post-stage indices (matching the Previews tab's stage labels); used by the stage detours.
pub const POST_STAGE_DOF: usize = 0;
pub const POST_STAGE_MB: usize = 1;

/// A per-eye snapshot of one post-effect stage's result texture. The debug texture + SRV are
/// created on the render thread (where the stage runs); the egui id is registered lazily on the UI
/// thread.
#[derive(Default)]
pub(super) struct StageCapture {
    created_desc: Option<(u32, u32, i32)>,
    texture: Option<ID3D11Texture2D>,
    pub(super) srv: Option<ID3D11ShaderResourceView>,
    pub(super) egui_id: Option<egui::TextureId>,
}

pub struct EguiDebugRenderState {
    /// Final back-buffer capture per Draw (eye): index 0 and index 1.
    pub(super) target_textures: [Option<(ID3D11Texture2D, egui::TextureId)>; 2],
    /// HDR scene (MainColor, pre-post) capture per eye -- the first column of the pipeline rows.
    pub(super) main_color_textures: [Option<(ID3D11Texture2D, egui::TextureId)>; 2],
    /// (w, h) the back-buffer captures were built for; recreate them when the back buffer resizes.
    target_size: Option<(u32, u32)>,
    /// (w, h, dxgi format) the MainColor captures were built for; recreate on change.
    main_color_desc: Option<(u32, u32, i32)>,
    /// Per-(stage, eye) captures of intermediate post-effect results: index `stage * 2 + eye`.
    pub(super) post_stage_captures: Vec<StageCapture>,
    /// Cache of engine SRV pointer -> egui texture id, for the live render-target thumbnails.
    srv_thumbnails: Vec<(usize, egui::TextureId)>,
}
impl EguiDebugRenderState {
    const fn new() -> Self {
        Self {
            target_textures: [None, None],
            main_color_textures: [None, None],
            target_size: None,
            main_color_desc: None,
            post_stage_captures: Vec::new(),
            srv_thumbnails: Vec::new(),
        }
    }

    /// Copy a post-effect stage's result texture into a per-(stage, eye) debug RT (render thread).
    fn capture_post_stage(
        &mut self,
        stage: usize,
        eye: usize,
        device: &jc3gi::graphics_engine::device::Device,
        context: &jc3gi::graphics_engine::device::Context,
        result: &jc3gi::graphics_engine::texture::Texture,
    ) {
        let idx = stage * 2 + eye;
        while self.post_stage_captures.len() <= idx {
            self.post_stage_captures.push(StageCapture::default());
        }
        let desc = (
            result.m_Width as u32,
            result.m_Height as u32,
            result.m_Format as i32,
        );
        let cap = &mut self.post_stage_captures[idx];
        unsafe {
            if cap.created_desc != Some(desc) {
                cap.texture = None;
                cap.srv = None;
                cap.egui_id = None;
                cap.created_desc = None;
                let mut texture: Option<ID3D11Texture2D> = None;
                if device
                    .m_Device
                    .CreateTexture2D(
                        &D3D11_TEXTURE2D_DESC {
                            Width: desc.0,
                            Height: desc.1,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT(desc.2),
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_DEFAULT,
                            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as _,
                            CPUAccessFlags: 0,
                            MiscFlags: 0,
                        },
                        None,
                        Some(&mut texture),
                    )
                    .is_err()
                {
                    return;
                }
                let Some(texture) = texture else {
                    return;
                };
                let mut srv: Option<ID3D11ShaderResourceView> = None;
                if device
                    .m_Device
                    .CreateShaderResourceView(&texture, None, Some(&mut srv))
                    .is_err()
                {
                    return;
                }
                cap.srv = srv;
                cap.texture = Some(texture);
                cap.created_desc = Some(desc);
            }
            if let Some(dst) = &cap.texture {
                EnterCriticalSection(context.m_Mutex);
                context.m_Context.CopyResource(dst, &result.m_Texture);
                LeaveCriticalSection(context.m_Mutex);
            }
        }
    }

    /// Get (registering+caching on first use) an egui texture id for an engine SRV.
    pub(super) fn thumbnail_id(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
        srv_raw: usize,
        srv: &ID3D11ShaderResourceView,
    ) -> egui::TextureId {
        if let Some((_, id)) = self.srv_thumbnails.iter().find(|(p, _)| *p == srv_raw) {
            return *id;
        }
        let id = renderer.register_user_texture(srv.clone());
        self.srv_thumbnails.push((srv_raw, id));
        id
    }

    pub(crate) fn prepare_if_necessary(&mut self, renderer: &mut egui_directx11::Renderer) {
        unsafe {
            let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
                return;
            };
            let Some(device) = ge.m_Device.as_mut() else {
                return;
            };

            // One eye's worth of the engine's render target (R8G8B8A8), recreated when its size
            // changes. Under single-pass double-wide the render target holds both eye-halves side by
            // side, so each per-eye capture texture is half its width -- the collapse's capture split
            // copies one full-width half in.
            if let Some(size) = crate::stereo::per_eye_render_size()
                && (self.target_size != Some(size)
                    || self.target_textures.iter().any(Option::is_none))
            {
                for slot in &mut self.target_textures {
                    if let Some((_, id)) = slot.take() {
                        renderer.unregister_user_texture(id);
                    }
                    *slot = Self::create_target(
                        device,
                        renderer,
                        size.0,
                        size.1,
                        DXGI_FORMAT_R8G8B8A8_UNORM,
                    );
                }
                self.target_size = Some(size);
            }

            // HDR scene (MainColor), matching its own format, recreated on size/format change.
            if let Some(mc) = ge.m_MainColorBuffer.as_ref() {
                let desc = (mc.m_Width as u32, mc.m_Height as u32, mc.m_Format as i32);
                if self.main_color_desc != Some(desc)
                    || self.main_color_textures.iter().any(Option::is_none)
                {
                    for slot in &mut self.main_color_textures {
                        if let Some((_, id)) = slot.take() {
                            renderer.unregister_user_texture(id);
                        }
                        *slot = Self::create_target(
                            device,
                            renderer,
                            desc.0,
                            desc.1,
                            DXGI_FORMAT(desc.2),
                        );
                    }
                    self.main_color_desc = Some(desc);
                }
            }
        }
    }

    fn create_target(
        device: &jc3gi::graphics_engine::device::Device,
        renderer: &mut egui_directx11::Renderer,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Option<(ID3D11Texture2D, egui::TextureId)> {
        // TODO: recreate on resize
        unsafe {
            let mut texture: Option<ID3D11Texture2D> = None;
            if let Err(e) = device.m_Device.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as _,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut texture),
            ) {
                tracing::error!("Failed to create texture: {e:?}");
                return None;
            }
            let texture = texture?;

            let mut srv: Option<ID3D11ShaderResourceView> = None;
            if let Err(e) = device
                .m_Device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
            {
                tracing::error!("Failed to create shader resource view: {e:?}");
                return None;
            }
            let srv = srv?;

            Some((texture, renderer.register_user_texture(srv)))
        }
    }

    pub fn texture(&self, index: usize) -> Option<&ID3D11Texture2D> {
        self.target_textures
            .get(index)?
            .as_ref()
            .map(|(texture, _)| texture)
    }

    pub fn main_color_texture(&self, index: usize) -> Option<&ID3D11Texture2D> {
        self.main_color_textures
            .get(index)?
            .as_ref()
            .map(|(texture, _)| texture)
    }

    fn uninstall(&mut self, renderer: &mut egui_directx11::Renderer) {
        for slot in self
            .target_textures
            .iter_mut()
            .chain(self.main_color_textures.iter_mut())
        {
            if let Some((_, texture_id)) = slot.take() {
                renderer.unregister_user_texture(texture_id);
            }
        }
        for (_, texture_id) in self.srv_thumbnails.drain(..) {
            renderer.unregister_user_texture(texture_id);
        }
        for cap in self.post_stage_captures.drain(..) {
            if let Some(id) = cap.egui_id {
                renderer.unregister_user_texture(id);
            }
        }
    }
}
pub static EGUI_DEBUG_RENDER_STATE: Mutex<EguiDebugRenderState> =
    Mutex::new(EguiDebugRenderState::new());

/// Capture a post-effect stage's result texture for the given eye -- called from the stage's detour
/// on the render thread, after the stage runs. `result` is the stage's slot result texture.
///
/// # Safety
/// `result` must be a valid engine `Texture` pointer (or null) from the post-effect slot array.
pub unsafe fn capture_post_stage(
    stage: usize,
    eye: usize,
    result: *mut jc3gi::graphics_engine::texture::Texture,
) {
    unsafe {
        let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let Some(result) = result.as_ref() else {
            return;
        };
        EGUI_DEBUG_RENDER_STATE
            .lock()
            .capture_post_stage(stage, eye, device, context, result);
    }
}

/// Capture the HDR scene buffer (MainColor) for `eye` at the start of the post chain (the exposure
/// histogram pass), before the chain reads and recycles it. Unlike a fixed grab at PostDraw, this
/// follows whatever instance the pipeline is currently using, so the "Scene" preview shows what this
/// dispatch actually rendered rather than a stale/recycled buffer.
pub fn capture_main_color(eye: usize) {
    unsafe {
        let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let Some(src) = ge.m_MainColorBuffer.as_ref() else {
            return;
        };
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        let Some(dst) = lock.main_color_texture(eye) else {
            return;
        };
        EnterCriticalSection(context.m_Mutex);
        context.m_Context.CopyResource(dst, &src.m_Texture);
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Register the debug render-state cleanup. Call once at init; it tears down the captured textures and
/// egui registrations at shutdown.
pub fn install() {
    crate::lifecycle::on_cleanup(|renderer| {
        if let Some(renderer) = renderer {
            EGUI_DEBUG_RENDER_STATE.lock().uninstall(renderer);
        }
    });
}

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye). Read by the
/// F10 capture composite so the recording window fuses the same way as the preview.
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub fn egui_debug_render(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();

    ui.checkbox(&mut cfg.stereo.enabled, "Stereo (double-Draw)")
        .on_hover_text("Issue a second game.Draw per frame; CClock::Update is gated to once/frame");

    ui.checkbox(
        &mut cfg.stereo.cameras,
        "Stereo cameras (per-eye IPD offset)",
    )
    .on_hover_text("Offset the active camera per eye so the two draws diverge");
    ui.add(egui::Slider::new(&mut cfg.stereo.ipd, 0.0..=100.0).text("IPD (m)"));

    ui.collapsing("FSR", |ui| {
        ui.checkbox(
            &mut cfg.fsr.enabled,
            "Anti-aliasing (replaces the engine SMAA)",
        );
        ui.checkbox(
            &mut cfg.fsr.jitter,
            "Temporal jitter (off = FSR blurs; A/B to confirm the jitter)",
        );
        ui.horizontal(|ui| {
            ui.label("Jitter sign (camera side):");
            let (sx, sy) = &mut cfg.fsr.jitter_sign;
            if ui.selectable_label(*sx > 0.0, "x+").clicked() {
                *sx = 1.0;
            }
            if ui.selectable_label(*sx < 0.0, "x-").clicked() {
                *sx = -1.0;
            }
            if ui.selectable_label(*sy > 0.0, "y+").clicked() {
                *sy = 1.0;
            }
            if ui.selectable_label(*sy < 0.0, "y-").clicked() {
                *sy = -1.0;
            }
        })
        .response
        .on_hover_text(
            "Must agree with the offset reported to the FSR dispatch, or fine detail \
                 pulses at the jitter cadence -- flip live to settle the convention",
        );
        ui.add(
            egui::Slider::new(&mut cfg.fsr.jitter_scale, 0.0..=1.0)
                .text("Jitter scale (diagnostic)"),
        );
        ui.horizontal(|ui| {
            let mut sharpen = cfg.fsr.sharpness.is_some();
            ui.checkbox(&mut sharpen, "Sharpening");
            match (sharpen, cfg.fsr.sharpness) {
                (true, None) => cfg.fsr.sharpness = Some(0.5),
                (false, Some(_)) => cfg.fsr.sharpness = None,
                _ => {}
            }
            if let Some(s) = cfg.fsr.sharpness.as_mut() {
                ui.add(egui::Slider::new(s, 0.0..=1.0).text("strength"));
            }
        });
        ui.checkbox(
            &mut cfg.fsr.motion_vectors,
            "Motion vectors (off = ghosts moving objects; A/B the decode)",
        );
        ui.checkbox(
            &mut cfg.fsr.mv_jitter_cancel,
            "MV jitter cancel (vectors carry the camera jitter; FSR wants them jitter-free)",
        )
        .on_hover_text(
            "The +/-0.5 px jitter wobble in the vectors flips FSR's history validation over \
                 steep depth gradients -- region-scale one-frame pops at the jitter cadence",
        );
        ui.checkbox(
            &mut cfg.fsr.mv_stereo_correction,
            "Stereo MV correction (re-anchor velocity at the per-eye camera)",
        )
        .on_hover_text(
            "The engine's velocity reprojects with the center camera's previous \
                 view-projection; each eye rasterizes with its own, so static pixels carry a \
                 spurious parallax vector and FSR flickers shadow edges per eye under motion \
                 (issue #10)",
        );
        ui.horizontal(|ui| {
            ui.label("MV sign:");
            let (sx, sy) = &mut cfg.fsr.mv_sign;
            if ui.selectable_label(*sx > 0.0, "x+").clicked() {
                *sx = 1.0;
            }
            if ui.selectable_label(*sx < 0.0, "x-").clicked() {
                *sx = -1.0;
            }
            if ui.selectable_label(*sy > 0.0, "y+").clicked() {
                *sy = 1.0;
            }
            if ui.selectable_label(*sy < 0.0, "y-").clicked() {
                *sy = -1.0;
            }
        });
    });

    ui.collapsing("Far field (#32, experimental)", |ui| {
        ui.checkbox(
            &mut cfg.far_field.enabled,
            "Enable near/far draw-list split (engine depth buckets)",
        )
        .on_hover_text(
            "Registers a depth-bucket boundary on the scene passes so their once-per-frame sort \
             produces a contiguous [near][far] list; the modes below window the draw onto one \
             run. Off restores the stock single bucket.",
        );
        ui.add(
            egui::Slider::new(&mut cfg.far_field.threshold_m, 50.0..=2000.0)
                .logarithmic(true)
                .text("Threshold (m)"),
        )
        .on_hover_text(
            "Instance-centre distance; large objects whose centre is past the threshold but \
             whose extent reaches nearer classify as far, so keep it conservative",
        );
        ui.horizontal(|ui| {
            ui.label("Far-regime types:");
            ui.text_edit_singleline(&mut cfg.far_field.gated_types)
                .on_hover_text(
                    "Registry type names (comma-separated) whose draws are inherently distant \
                     and skip with the far field, with no distance split — find candidates by \
                     bisecting the Diagnostics tab's render-block-type registry",
                );
        });
        let gated = crate::far_field::gated_type_names();
        if !gated.is_empty() {
            ui.label(format!("gated: {}", gated.join(", ")));
        }
        // Keep the IsEnabled overrides in sync with the list (and drop them all when the split is
        // disabled); `sync_type_gates` no-ops when nothing changed.
        crate::far_field::sync_type_gates(if cfg.far_field.enabled {
            &cfg.far_field.gated_types
        } else {
            ""
        });
        ui.horizontal(|ui| {
            ui.label("Mode:");
            use crate::config::FarFieldMode;
            for (mode, label, hover) in [
                (
                    FarFieldMode::Collect,
                    "Collect",
                    "Split + counters only; draw everything",
                ),
                (
                    FarFieldMode::SkipFar,
                    "Skip far",
                    "Far field vanishes on both eyes",
                ),
                (
                    FarFieldMode::SkipNear,
                    "Skip near",
                    "Far field in isolation",
                ),
                (
                    FarFieldMode::SkipFarEye1,
                    "Skip far, eye 1",
                    "The sharing candidate: eye 1 skips the far run",
                ),
                (
                    FarFieldMode::Share,
                    "Share",
                    "Render the far field once per frame (a third, far-only dispatch) and \
                     composite its G-buffer under both eyes; requires stereo",
                ),
            ] {
                // `Share` needs the two per-eye dispatches it composites into, which the single-pass
                // collapse does away with; the two are mutually exclusive.
                let blocked = mode == FarFieldMode::Share && collapse_configured(&cfg);
                ui.add_enabled_ui(!blocked, |ui| {
                    ui.selectable_value(&mut cfg.far_field.mode, mode, label)
                        .on_hover_text(if blocked {
                            "Unavailable while the single-pass collapse is on: the collapse renders \
                             both eyes in one walk, so there are no per-eye dispatches to share a \
                             far G-buffer between"
                        } else {
                            hover
                        });
                });
            }
        });
        if ui
            .button("Dump split state (log)")
            .on_hover_text(
                "Log the next frame's per-pass classification state — buckets, keys, and the \
                 terrain blocks' placement fields — at INFO under the far_field target",
            )
            .clicked()
        {
            crate::far_field::request_dump(16);
        }
        far_field_stats_table(ui);
    });

    // The stereo render corrections, grouped by subsystem -- normally on; toggle off to reproduce
    // the artifact each fixes. Collapsed by default to keep the tab scannable. (The investigation
    // probes live in the Diagnostics tab.)
    ui.collapsing("Shadows", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fix_shadow_cascade_anchor,
            "Cascade anchor (the visible per-eye shadow mismatch; A/B via Present eye 0)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_shadow_fit,
            "Widen fit FOV (cascades cover both eyes; fixes distant per-eye shadow disagreement + \
             crawl)",
        );
        ui.checkbox(
            &mut cfg.stereo.stabilize_shadow_fit,
            "Stabilize fit vs head tilt (yaw-only cascade centre; fixes shadows shifting/scaling \
             when you look around)",
        );
    });

    ui.collapsing("Depth reconstruction", |ui| {
        ui.checkbox(
            &mut cfg.stereo.reconstruct_offaxis_inverse,
            "Off-axis depth reconstruction (per-eye inverse for deferred/SS passes; fixes \
             specular/SSR/shadow reconstruction divergence)",
        );
    });

    ui.collapsing("Clustered lighting", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fix_clustered_light_frustum,
            "Off-axis froxel tile bounds (replaces symmetric cb1 with per-eye projection-derived \
             bounds; fixes blocky 64px lighting tiles in VR)",
        );
    });

    ui.collapsing("Cross-eye consistency", |ui| {
        ui.checkbox(
            &mut cfg.stereo.dedupe_post_block,
            "Dedupe world post block (eye 1 otherwise runs the post chain + FSR twice)",
        );
        ui.checkbox(
            &mut cfg.stereo.drain_draw_fragment,
            "Drain draw-dispatch fragment between eyes (open-world crash fix)",
        );
        ui.checkbox(
            &mut cfg.stereo.defer_frame_tail,
            "Defer the frame tail to a worker (overlap next sim with the GPU tail)",
        )
        .on_hover_text(
            "Moves the final drain, VR blit/submit, and mirror onto a tail thread so the next \
             frame's sim starts immediately. A/B with the profiler's 'GPU idle' number.",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_frame_counters,
            "Restore frame counters between eyes (fixes jitter/parity flicker)",
        );
        ui.add_enabled(
            cfg.stereo.restore_frame_counters,
            egui::Checkbox::new(
                &mut cfg.stereo.share_prepasses,
                "Share view-independent pre-passes across eyes (reflections, cloud shadows, \
                 sun-shadow atlas, water sim rendered once)",
            ),
        )
        .on_hover_text(
            "On eye 1, reuse eye 0's shadow atlas / reflection proxies / water sim instead of \
             re-rendering them. Requires 'Restore frame counters'. If distant reflections or \
             shadows look wrong in one eye, turn this off.",
        );
        ui.checkbox(
            &mut cfg.stereo.force_smaa_1x,
            "Force SMAA 1x (T2X's shared history ghosts across eyes)",
        );
        ui.checkbox(
            &mut cfg.stereo.force_ssao_first_pass,
            "Force SSAO first-pass per eye (stops cross-eye AO history blend)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_ssao_history,
            "Restore SSAO history between eyes (pin the AO temporal slot so both eyes match)",
        );
        ui.checkbox(
            &mut cfg.stereo.restore_gi_cascade,
            "Restore GI cascade between eyes (pin the LPV cascade so both eyes match)",
        );
    });

    ui.collapsing("Culling & geometry", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.widen_cull_frustum,
                "Widen scene cull frustum (covers both eyes; stops outer-edge void/pop-in)",
            );
            ui.add_enabled(
                cfg.stereo.widen_cull_frustum,
                egui::Slider::new(&mut cfg.stereo.cull_fov_padding, 0.0..=0.75)
                    .text("pad")
                    .fixed_decimals(2),
            )
            .on_hover_text(
                "Extra fraction to widen the cull frustum on every side (incl. vertical); raise if \
                 geometry still pops in at the edges when flying",
            );
        });
        ui.add(
            egui::Slider::new(&mut cfg.stereo.cull_size_fov_deg, 0.0..=90.0)
                .text("Size-cull FOV (deg)")
                .fixed_decimals(0),
        )
        .on_hover_text(
            "FOV the screen-space size cull uses (overrides the injected 90 deg on the cull \
             camera); lower keeps more small/distant geometry and vehicle parts. 0 = leave alone",
        );
        ui.checkbox(
            &mut cfg.stereo.disable_bfbc_occlusion,
            "Disable software occlusion (drops centre-viewpoint occluder culling; fixes peripheral \
             culling an offset eye can see past)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_terrain_cull,
            "Widen terrain patch cull (rebuild the cull frustum planes; fixes terrain patch holes \
             at the edges when flying)",
        );
        ui.checkbox(
            &mut cfg.stereo.widen_model_cull,
            "Widen model cull (active-camera frustum; fixes buildings popping at the edges)",
        );
        ui.checkbox(
            &mut cfg.stereo.invalidate_terrain_cb,
            "Invalidate terrain tess CB between eyes (forces eye 1 to re-upload its own off-axis \
             projection; fixes distant tessellated terrain sheared to eye 0)",
        );
    });

    ui.collapsing("Shader patches", |ui| {
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.patch_shadow_pcf_hash,
                "Sun-shadow PCF screen-hash (kills per-eye shimmer + foliage grain)",
            );
            let patched = shader::patched_count();
            ui.label(if patched == 0 {
                "(0 patched -- click Reload shaders)".to_string()
            } else {
                format!("({patched} sites patched)")
            });
        });
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut cfg.stereo.patch_lod_dissolve,
                "Jitter-unstable LOD dissolve (only matters with FSR jitter on)",
            );
            let patched = shader::dissolve_patched_count();
            ui.label(if patched == 0 {
                "(0 patched -- click Reload shaders)".to_string()
            } else {
                format!("({patched} sites patched)")
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                shader::request_reload();
            }
            ui.label(
                "re-creates all shaders so the shader patches take effect (F11 toggles + reloads)",
            );
        });
    });

    // Single-pass stereo (experimental, off by default): render the G-buffer geometry once, emitting
    // both eyes via instancing + SV_ViewportArrayIndex routing, instead of the double-draw. The
    // pipeline is under construction; the census dry-run below is safe (no rendering change) and
    // reports how the vertex-shader rewriter fares against the game's real shader set.
    ui.collapsing("Single-pass stereo (experimental)", |ui| {
        use crate::stereo::single_pass::{self, Capability};
        // All-in/all-out toggle: flip the whole work-in-progress configuration together, so the one
        // button is the normal way to turn the feature on or off. The individual levers below stay for
        // bring-up. Reloads either way -- on to apply the patches, off to restore the pristine shaders.
        let enabled = cfg.stereo.single_pass;
        let toggle_label = if enabled {
            "⚡ Disable single-pass stereo"
        } else {
            "⚡ Enable single-pass stereo"
        };
        if ui
            .button(toggle_label)
            .on_hover_text(
                "Flips single-pass, dual-eye, collapse, double-wide, and native resolution on or off \
                 together, clears the census dry-run, and reloads the shaders.",
            )
            .clicked()
        {
            let on = !enabled;
            cfg.stereo.single_pass = on;
            cfg.stereo.single_pass_dual_eye = on;
            cfg.stereo.single_pass_collapse = on;
            cfg.stereo.single_pass_double_wide = on;
            cfg.stereo.single_pass_reproject = on;
            cfg.stereo.single_pass_terrain = on;
            cfg.stereo.single_pass_patch_dryrun = false;
            cfg.vr.native_resolution = on;
            if on && cfg.far_field.mode == crate::config::FarFieldMode::Share {
                cfg.far_field.mode = crate::config::FarFieldMode::Collect;
            }
            shader::request_reload();
        }
        ui.separator();
        ui.checkbox(
            &mut cfg.stereo.single_pass,
            "Enable single-pass stereo (experimental)",
        )
        .on_hover_text(
            "Master switch. Renders the G-buffer once with stereo-rewritten vertex shaders. \
             Forced inert without the DXVK viewport-routing capability (below).",
        );
        ui.checkbox(
            &mut cfg.stereo.single_pass_patch_dryrun,
            "Census only (dry-run: patch + tally, do not substitute -- safe)",
        )
        .on_hover_text(
            "Runs the vertex-shader stereo rewrite on every shader at creation and counts the \
             outcomes, without changing rendering. Validates the rewriter against real shaders.",
        );
        ui.separator();
        ui.label("Enable top-to-bottom; all are needed together for a clean image:");
        ui.add_enabled_ui(cfg.stereo.single_pass, |ui| {
            ui.checkbox(
                &mut cfg.stereo.single_pass_dual_eye,
                "Dual-eye: distinct per-eye cb13 + eye-half viewports + instance doubling",
            )
            .on_hover_text(
                "Makes the eyes diverge. On its own (no double-wide, no collapse) each eye \
                 renders into half of a per-eye target -- squished; a bisection step.",
            );
            ui.add_enabled_ui(cfg.stereo.single_pass_dual_eye, |ui| {
                ui.checkbox(
                    &mut cfg.stereo.single_pass_double_wide,
                    "Double-wide render target (full per-eye resolution)",
                )
                .on_hover_text(
                    "Renders the scene targets at 2x per-eye width so each eye-half is full \
                     resolution instead of squished. Needs Collapse and native resolution on.",
                );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass_collapse,
                        "Collapse to a single game.Draw walk (the actual perf win; riskiest)",
                    )
                    .on_hover_text(
                        "One walk renders both eyes: centered camera, no between-eye restore, the \
                         back buffer split into the two eye textures. Without double-wide each eye \
                         is squished/half-filled; unpatched geometry and the HUD reach the left eye \
                         only. Turns the far-field Share mode off -- the two are exclusive.",
                    )
                    .changed()
                    && cfg.stereo.single_pass_collapse
                    && cfg.far_field.mode == crate::config::FarFieldMode::Share
                {
                    cfg.far_field.mode = crate::config::FarFieldMode::Collect;
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass_reproject,
                        "Reproject the no-cb0 scene families (NPCs, props, buildings, roads)",
                    )
                    .on_hover_text(
                        "Rewrites the baked-WVP scene shaders (characters, props, buildings, roads) \
                         to post-multiply their clip by the per-eye M_eye, instead of leaving them \
                         double-drawn. Reloads shaders to apply. Sky/UI/post are excluded.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass_terrain,
                        "Single-pass the tessellated terrain (VS -> HS -> DS)",
                    )
                    .on_hover_text(
                        "Rides the eye index through the terrain tessellation pipeline: the vertex \
                         shader writes it on the free TEXCOORD3.z lane, the hull forwards it, and the \
                         domain reprojects its clip by the per-eye M_eye. Covers the DrawIndexed \
                         terrain passes; the GPU-indirect near passes stay double-drawn. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass_tree_impostors,
                        "Single-pass the tree impostors (far-distance billboards)",
                    )
                    .on_hover_text(
                        "Reprojects the treeimpostor* vertex shaders by the per-eye M_eye, like the \
                         scene families. They draw non-instanced with no GPU-indirect path, so this \
                         covers them completely. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.label("Render-block re-issue (blind-implemented, unvalidated -- no shader reload):");
                ui.checkbox(
                    &mut cfg.stereo.single_pass_bark,
                    "Bark (tree trunks/branches)",
                )
                .on_hover_text(
                    "Re-issues CRenderBlockBark's Draw/DrawZ once per eye with its baked cb1 \
                     view-projection reprojected by M_eye. Covers its plain/instanced/GPU-indirect \
                     draw kinds.",
                );
                ui.checkbox(&mut cfg.stereo.single_pass_foliage, "Foliage (grass)")
                    .on_hover_text(
                        "Re-issues CRenderBlockFoliage's Draw once per eye with its baked cb2 \
                         view-projection reprojected by M_eye. Does not fix the separate forward-\
                         lighting black-grass issue.",
                    );
                ui.checkbox(
                    &mut cfg.stereo.single_pass_occluder,
                    "Occluder (depth-prime boxes)",
                )
                .on_hover_text(
                    "Re-issues CRenderBlockOccluder's DrawZ once per eye with its baked cb1 \
                     view-projection reprojected by M_eye, priming each eye's depth with its own \
                     projection.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass_instanced_per_eye,
                    "Already-instanced draws per eye (buildings, vegetation)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstanced whose patched shader is bound once per eye, \
                     with both cb13 eye slots and both viewport slots pinned to that eye, so the \
                     game's own instance ids stop being read as an eye parity. Off, the batch is \
                     split alternately between the eyes -- the building flicker.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass_uniform_viewport_slots,
                    "Uniform viewport slots outside the G-buffer",
                )
                .on_hover_text(
                    "Puts both viewport slots back to one region once the G-buffer range ends, so an \
                     instanced draw with a patched shader in a pass that is not eye-split (shadows, \
                     reflections, post) keeps its odd-numbered instances instead of routing them into \
                     the other eye's half. Slot 0 is never changed.",
                );
            });
        });

        let capability = single_pass::probe_if_needed();
        ui.label(match capability {
            Capability::Supported => "Viewport routing: supported ✓",
            Capability::Unsupported => "Viewport routing: UNSUPPORTED (single-pass will stay inert)",
            Capability::Unprobed => "Viewport routing: not yet probed (no device seen)",
        });

        let (patched, no_refs, deferred, errored) = (
            single_pass::patched_count(),
            single_pass::no_refs_count(),
            single_pass::deferred_count(),
            single_pass::errored_count(),
        );
        if patched + no_refs + deferred + errored == 0 {
            ui.label("Census: 0 shaders seen -- enable a mode above, then click Reload shaders.");
        } else {
            ui.label(format!(
                "Census: {patched} patched, {no_refs} no per-eye refs, {deferred} instance-id deferred, {errored} errored"
            ));
            if errored > 0 {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "errored > 0: a shader the offline corpus did not cover -- check the log",
                );
            }
        }
        let (hs_forwarded, ds_reprojected) = single_pass::terrain_counts();
        ui.label(format!(
            "Terrain: {hs_forwarded} hull forwarded, {ds_reprojected} domain reprojected"
        ));
        let s = single_pass::substitution_stats();
        ui.label(format!(
            "Recorded VS (doubled): {} | CreateVertexShader: pending {}, re-acquired [patched {}, already-cb13 {}, no-refs {}, err {}]",
            s.recorded_vs,
            s.cvs_pending,
            s.cvs_reacq_patched,
            s.cvs_reacq_cb13,
            s.cvs_reacq_no_refs,
            s.cvs_reacq_err,
        ));
        instanced_exposure_readout(ui);
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                shader::request_reload();
            }
            ui.label("re-runs the census over a fresh shader-creation pass");
        });
    });

    // Resolution levers for issue #8's pixelation/large-tile artifact around lights and explosions:
    // the engine's reduced-resolution fog/particle/spotlight passes, whose coarse grids VR's wide
    // FOV magnifies. All default off (not headset-verifiable; particles can hide content).
    ui.collapsing("Resolution (pixelation)", |ui| {
        ui.checkbox(
            &mut cfg.stereo.fog_full_res,
            "Fog volume full-res (coarse froxel depth buffer; applies at next resolution change)",
        )
        .on_hover_text(
            "No-ops the half-res multiplies in the fog block's ResizeTextures so the coarse \
             volumetric-depth buffer is recreated at full resolution. Most likely fix for the \
             light/explosion tiles. Only re-runs on a resolution change.",
        );
        ui.checkbox(
            &mut cfg.stereo.particles_full_res,
            "Particles full-res (route to the full-res transparent pass) -- RISKY, A/B live",
        )
        .on_hover_text(
            "Clears the particle block type's low-res routing flags so particles draw in the \
             full-res transparent pass. The full-res pass always draws, so particles reroute rather \
             than vanish -- but verify live: a family that does not survive the reroute could look \
             wrong. Applies one frame ahead.",
        );
        ui.checkbox(
            &mut cfg.stereo.spotlight_full_res,
            "Spotlight volumetrics full-res (engine's full-res branch)",
        )
        .on_hover_text(
            "Scopes g_EnableLowResSpotLightVolume off around the light gather so spot-light cones \
             render at full resolution into the main setup. Lowest-risk lever.",
        );
    });

    ui.collapsing("Foveation (#29, experimental)", |ui| {
        ui.checkbox(
            &mut cfg.foveation.enabled,
            "Enable static foveated rendering",
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.inner_fraction, 0.0..=1.0)
                .text("Inner radius (fraction of half-diagonal, full-res inside)"),
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.outer_fraction, 0.0..=1.5)
                .text("Outer radius (drop reaches max here)"),
        );
        ui.add(
            egui::Slider::new(&mut cfg.foveation.max_drop, 0.0..=1.0)
                .text("Max peripheral drop fraction"),
        );
        ui.horizontal(|ui| {
            ui.label("Foveated pass range (RenderPassId):");
            ui.add(egui::DragValue::new(&mut cfg.foveation.foveal_first_pass).range(0..=0xFF));
            ui.label("..=");
            ui.add(egui::DragValue::new(&mut cfg.foveation.foveal_last_pass).range(0..=0xFF));
        });
        ui.checkbox(
            &mut cfg.foveation.debug_show_mask,
            "Debug: paint dropped pixels magenta (visualize the mask)",
        );
        ui.label(
            "Drops a dithered radial fraction of peripheral pixels before shading, then \
             reconstructs them. Off by default; needs in-headset tuning.",
        );
    });

    ui.collapsing("Post-FX (reprojection passes, both eyes)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur,
            "Skip MotionBlur::Apply (whole pass)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur_recon,
            "Skip MotionBlur recon (if pass not skipped)",
        );
        ui.checkbox(
            &mut cfg.post_fx.dof_no_reproject,
            "DoF: plain composite, no reprojection (keeps picture)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_dof,
            "Skip DepthOfField::Apply (washes out!)",
        );
    });

    ui.collapsing("Post-FX stages (skip to bisect)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_histogram,
            "Exposure histogram (stalls auto-exposure)",
        );
        ui.checkbox(&mut cfg.post_fx.skip_glare, "Glare / bloom");
        ui.checkbox(&mut cfg.post_fx.skip_fade, "Fade");
        ui.checkbox(&mut cfg.post_fx.skip_sun_halo, "Sun halo");
        ui.checkbox(
            &mut cfg.post_fx.skip_player_damage,
            "Player-damage vignette",
        );
    });
}

/// Whether the config asks for the single-pass collapse, ignoring the runtime gates
/// ([`crate::stereo::single_pass::collapse_active`] also requires the device capability and a live
/// session). The UI needs the *intent*, so a mutually-exclusive option greys out as soon as the boxes
/// are ticked rather than only once the collapse is running.
fn collapse_configured(cfg: &config::Config) -> bool {
    cfg.stereo.single_pass && cfg.stereo.single_pass_dual_eye && cfg.stereo.single_pass_collapse
}

/// How much of the collapse's frame the already-instanced eye-parity case covers, and how much of that
/// the per-eye re-issue is handling. The case: an instanced draw's instance ids are the game's own, so
/// the patched shader's `SV_InstanceID & 1` reads them as an eye parity and the batch would be split
/// between the eyes (or, at one instance, land in the left eye alone). *Handled* draws were re-issued
/// once per eye; *exposed* ones still carry the artifact. The per-shader list says which families are
/// paying the extra submissions.
fn instanced_exposure_readout(ui: &mut egui::Ui) {
    let report = single_pass::instanced_exposure();
    if report.frames == 0 {
        ui.label("Instanced eye-parity: no single-pass geometry frames measured yet.");
        return;
    }
    let last = report.last_frame;
    ui.label(format!(
        "Instanced eye-parity: {} handled, {} exposed of {} DrawIndexedInstanced last frame | mean \
         over {} frames: {:.1} handled ({:.1} instances) + {:.1} exposed ({:.1} single-instance, \
         {:.1} split batches) of {:.1}, peak {} instances",
        last.handled,
        last.affected,
        last.total,
        report.frames,
        report.mean_handled,
        report.mean_handled_instances,
        report.mean_affected,
        report.mean_affected_single_instance,
        report.mean_affected_multi_instance,
        report.mean_total,
        report.peak_instances,
    ))
    .on_hover_text(
        "Draws where a patched vertex shader was bound inside the G-buffer range under the collapse, \
         excluding the mod's own promoted and per-eye re-issued draws. Handled = re-issued once per \
         eye with cb13 and the viewport pinned to that eye; exposed = still split by instance parity, \
         which is what the toggle above turns into handled.",
    );
    ui.label(format!(
        "By range: in-range {} patched + {} unpatched | out-of-range {} patched ({} instances) + {} \
         unpatched | mean out-of-range patched over {} frames: {:.1} draws, {:.1} instances",
        last.handled + last.affected,
        last.in_range_unpatched,
        last.out_of_range_patched,
        last.out_of_range_patched_instances,
        last.out_of_range_unpatched,
        report.frames,
        report.mean_out_of_range_patched,
        report.mean_out_of_range_patched_instances,
    ))
    .on_hover_text(
        "Every DrawIndexedInstanced of the frame, split by whether a patched vertex shader was bound \
         and whether the draw was inside the G-buffer range. Only the in-range patched draws are \
         eye-split; the out-of-range patched ones still write SV_ViewportArrayIndex from instance \
         parity, so they need both viewport slots to hold the same region.",
    );
    let offenders = single_pass::instanced_offenders(8);
    if offenders.is_empty() {
        return;
    }
    ui.collapsing("Instanced patched-shader draws by shader", |ui| {
        for offender in offenders {
            let name = offender
                .name
                .unwrap_or_else(|| format!("<unnamed {:#x}>", offender.shader));
            ui.label(format!(
                "{name}: in-range {} draws / {} instances, out-of-range {} draws / {} instances",
                offender.draws,
                offender.instances,
                offender.out_of_range_draws,
                offender.out_of_range_instances
            ));
        }
    });
}

/// The per-pass near/far split counters, freshest first. Entries older than a second (passes that
/// stopped drawing, or the split disabled) are dropped from view.
fn far_field_stats_table(ui: &mut egui::Ui) {
    let mut stats = crate::far_field::stats_snapshot();
    stats.retain(|(_, s)| s.updated.elapsed().as_secs_f32() < 1.0);
    if stats.is_empty() {
        ui.label("(no split passes drawn in the last second)");
        return;
    }
    let (near, far) = stats.iter().fold((0u64, 0u64), |(n, f), (_, s)| {
        (n + u64::from(s.near), f + u64::from(s.far))
    });
    ui.label(format!(
        "Total: {near} near / {far} far ({:.0}% far)",
        100.0 * far as f64 / (near + far).max(1) as f64
    ));
    egui::Grid::new("far_field_stats")
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Pass").strong());
            ui.label(egui::RichText::new("Near").strong());
            ui.label(egui::RichText::new("Far").strong());
            ui.label(egui::RichText::new("Windowed").strong());
            ui.end_row();
            for (id, s) in stats {
                ui.label(format!("{:#04X} {}", id, render_pass_name(id)));
                ui.label(s.near.to_string());
                ui.label(s.far.to_string());
                ui.label(if s.windowed { "yes" } else { "" });
                ui.end_row();
            }
        });
}

/// The engine's debug name for a pass id, via `GetRenderPassName`.
fn render_pass_name(id: i16) -> &'static str {
    if !(0..0x9D).contains(&i32::from(id)) {
        return "(out of range)";
    }
    // SAFETY: the id is in the enum's verified range, and the engine returns static strings.
    unsafe {
        let pass = std::mem::transmute::<i32, jc3gi::graphics_engine::render_engine::RenderPassId>(
            i32::from(id),
        );
        let ptr = jc3gi::graphics_engine::render_engine::GetRenderPassName(pass);
        if ptr.is_null() {
            return "(null)";
        }
        std::ffi::CStr::from_ptr(ptr.cast())
            .to_str()
            .unwrap_or("(non-utf8)")
    }
}
