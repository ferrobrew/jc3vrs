//! The Render tab: the core stereo widgets plus the per-eye preview grid, and the render-thread
//! capture state that feeds it.

use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use windows::{
    Win32::{
        Graphics::{
            Direct3D11::{
                D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                ID3D11ShaderResourceView, ID3D11Texture2D,
            },
            Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface,
};

use crate::debug::camera::{CAMERA_SNAPSHOTS, CameraSnapshot};

use crate::config;

use super::camera::matrix_grid;

/// Labels for the post-effect stages captured per eye, in chain order.
const POST_STAGE_LABELS: [&str; 2] = ["after DoF", "after MB"];
/// Post-stage indices (must match POST_STAGE_LABELS); used by the stage detours.
pub const POST_STAGE_DOF: usize = 0;
pub const POST_STAGE_MB: usize = 1;

/// A per-eye snapshot of one post-effect stage's result texture. The debug texture + SRV are
/// created on the render thread (where the stage runs); the egui id is registered lazily on the UI
/// thread.
#[derive(Default)]
struct StageCapture {
    created_desc: Option<(u32, u32, i32)>,
    texture: Option<ID3D11Texture2D>,
    srv: Option<ID3D11ShaderResourceView>,
    egui_id: Option<egui::TextureId>,
}

pub struct EguiDebugRenderState {
    /// Final back-buffer capture per Draw (eye): index 0 and index 1.
    target_textures: [Option<(ID3D11Texture2D, egui::TextureId)>; 2],
    /// HDR scene (MainColor, pre-post) capture per eye -- the first column of the pipeline rows.
    main_color_textures: [Option<(ID3D11Texture2D, egui::TextureId)>; 2],
    /// (w, h) the back-buffer captures were built for; recreate them when the back buffer resizes.
    target_size: Option<(u32, u32)>,
    /// (w, h, dxgi format) the MainColor captures were built for; recreate on change.
    main_color_desc: Option<(u32, u32, i32)>,
    /// Per-(stage, eye) captures of intermediate post-effect results: index `stage * 2 + eye`.
    post_stage_captures: Vec<StageCapture>,
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
    fn thumbnail_id(
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

    fn prepare_if_necessary(&mut self, renderer: &mut egui_directx11::Renderer) {
        unsafe {
            let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
                return;
            };
            let Some(device) = ge.m_Device.as_mut() else {
                return;
            };

            // Final back buffer (R8G8B8A8), recreated when its size changes.
            if let Some(back_buffer) = device.m_BackBuffer.as_ref() {
                let size = (back_buffer.m_Width as u32, back_buffer.m_Height as u32);
                if self.target_size != Some(size)
                    || self.target_textures.iter().any(Option::is_none)
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

/// Preview thumbnail width (px) in the Render tab; user-controllable via a slider.
static PREVIEW_WIDTH: Mutex<f32> = Mutex::new(700.0);

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
        EGUI_DEBUG_RENDER_STATE.lock().uninstall(renderer);
    });
}

fn gate_checkbox(ui: &mut egui::Ui, flag: &std::sync::atomic::AtomicBool, label: &str) {
    let mut v = flag.load(Ordering::Relaxed);
    if ui.checkbox(&mut v, label).changed() {
        flag.store(v, Ordering::Relaxed);
    }
}

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye). Read by the
/// F10 capture composite so the recording window fuses the same way as the preview.
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

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

pub fn egui_debug_render(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    // Scope the CONFIG lock to just the core stereo widgets -- it must never be held at the same
    // time as EGUI_DEBUG_RENDER_STATE (lock ordering), so it is dropped before the capture/thumbnail
    // code.
    {
        let mut cfg = config::CONFIG.lock();

        ui.checkbox(&mut cfg.stereo.enabled, "Stereo (double-Draw)")
            .on_hover_text(
                "Issue a second game.Draw per frame; CClock::Update is gated to once/frame",
            );

        ui.checkbox(
            &mut cfg.stereo.cameras,
            "Stereo cameras (per-eye IPD offset)",
        )
        .on_hover_text("Offset the active camera per eye so the two draws diverge");
        ui.add(egui::Slider::new(&mut cfg.stereo.ipd, 0.0..=100.0).text("IPD (m)"));

        egui::CollapsingHeader::new("FSR")
            .default_open(false)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut cfg.fsr.enabled,
                    "Anti-aliasing (replaces the engine SMAA)",
                );
                ui.checkbox(
                    &mut cfg.fsr.jitter,
                    "Temporal jitter (off = FSR blurs; A/B to confirm the jitter)",
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
    }

    let preview_width = {
        let mut w = PREVIEW_WIDTH.lock();
        ui.add(egui::Slider::new(&mut *w, 48.0..=4096.0).text("Preview size (px)"));
        *w
    };

    let mut state = EGUI_DEBUG_RENDER_STATE.lock();
    state.prepare_if_necessary(renderer);

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

            ui.collapsing("Per-eye render camera (differences in yellow)", |ui| {
                let (s0, s1) = {
                    let g = CAMERA_SNAPSHOTS.lock();
                    (g[0], g[1])
                };
                ui.columns(2, |cols| {
                    show_camera_snapshot(&mut cols[0], "Eye 0", &s0, &s1);
                    show_camera_snapshot(&mut cols[1], "Eye 1", &s1, &s0);
                });
            });
        });
}

fn show_camera_snapshot(
    ui: &mut egui::Ui,
    label: &str,
    snap: &CameraSnapshot,
    other: &CameraSnapshot,
) {
    ui.label(egui::RichText::new(label).strong());
    if !snap.valid {
        ui.label("(no capture)");
        return;
    }
    ui.label(format!("cam ptr: {:#x}", snap.camera_ptr));

    const FLAG_NAMES: [(u8, &str); 6] = [
        (0x01, "OffCenter"),
        (0x02, "ScreenshotSeries"),
        (0x04, "Ortho"),
        (0x08, "ComputeView"),
        (0x10, "DirtyProj"),
        (0x20, "IsRenderCam"),
    ];
    let active: Vec<&str> = FLAG_NAMES
        .iter()
        .filter(|(b, _)| snap.state_bits & b != 0)
        .map(|(_, n)| *n)
        .collect();
    let flag_text = format!("flags {:#04x}: {}", snap.state_bits, active.join(" | "));
    if other.valid && snap.state_bits != other.state_bits {
        ui.colored_label(egui::Color32::YELLOW, flag_text);
    } else {
        ui.label(flag_text);
    }

    ui.label(format!(
        "offcenter: tiles={} x={} y={}",
        snap.offcenter_tiles, snap.offcenter_tile_x, snap.offcenter_tile_y
    ));
    ui.label(format!("fov={:.4}  aspect={:.4}", snap.fov, snap.aspect));
    ui.label(format!("near={:.3}  far={:.1}", snap.near, snap.far));
    ui.label(format!("size={}x{}", snap.width, snap.height));

    let other_proj = other.valid.then_some(&other.projection);
    let other_view = other.valid.then_some(&other.view);
    let other_vpf = other.valid.then_some(&other.view_proj_f);
    matrix_grid(
        ui,
        &format!("proj_{label}"),
        "m_Projection:",
        &snap.projection,
        other_proj,
    );
    matrix_grid(
        ui,
        &format!("view_{label}"),
        "m_View:",
        &snap.view,
        other_view,
    );
    matrix_grid(
        ui,
        &format!("vpf_{label}"),
        "m_ViewProjectionF:",
        &snap.view_proj_f,
        other_vpf,
    );
    let other_tf = other.valid.then_some(&other.transform);
    matrix_grid(
        ui,
        &format!("tf_{label}"),
        "m_TransformF:",
        &snap.transform,
        other_tf,
    );
}
