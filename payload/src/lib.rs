use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::HMODULE,
    Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
            ID3D11ShaderResourceView, ID3D11Texture2D,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
    },
    System::{
        LibraryLoader::DisableThreadLibraryCalls,
        SystemServices::DLL_PROCESS_ATTACH,
        Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    UI::Input::KeyboardAndMouse::{VK_F5, VK_F6},
};

use windows::core::Interface;

use crate::egui_impl::EguiState;

pub mod egui_impl;
pub mod module;
pub mod util;

mod crash;
mod hooks;
mod logging;

/// When enabled, the manual Draw driver issues a second `game.Draw` per real frame (the stereo
/// "second eye"), and the `CClock::Update` detour gates the clock to once per real frame so the
/// SPF smoother isn't polluted (avoids slow-mo). Toggle via the Render tab.
pub static STEREO: AtomicBool = AtomicBool::new(false);

/// Which Draw (eye) the manual driver is currently dispatching: 0 or 1. Set before each
/// `game.Draw`, read by the post-draw capture to route the back buffer into the matching RT.
pub static DRAW_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Giant-IPD stereo camera test: offset the active camera per eye so the two renders are visually
/// distinct, confirming two independent draws. Toggle via the Render tab.
pub static STEREO_CAMERAS: AtomicBool = AtomicBool::new(true);
/// Inter-pupillary distance (metres) for the stereo camera offset; large for the visual test.
pub static STEREO_IPD: Mutex<f32> = Mutex::new(2.0);
/// Which eye reaches the screen in stereo double-Draw: false = eye 1 (default), true = eye 0. Lets
/// each eye's render be compared live, bypassing the (flaky) per-eye capture.
pub static PRESENT_EYE_0: AtomicBool = AtomicBool::new(false);

/// Render-call trace: when > 0, pipeline hooks append a JSON record per call to TRACE_LOG;
/// decremented once per real frame and dumped as NDJSON when it reaches 0. Driven by the "Dump
/// render trace" button.
pub static TRACE_FRAMES: AtomicI32 = AtomicI32::new(0);
static TRACE_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Absolute path of the most recent trace dump, shown in the UI so it's findable.
static LAST_TRACE_PATH: Mutex<Option<String>> = Mutex::new(None);

/// Per-eye GPU-command counters: reset at each eye's `draw_begin`, read at `draw_end`. Bumped by the
/// detours on the engine's draw wrappers (the draw counters) and `Dispatch`/`DispatchIndirect` (the
/// dispatch counter).
pub static DRAW_CALLS: AtomicUsize = AtomicUsize::new(0);
pub static DRAW_INDEXED_CALLS: AtomicUsize = AtomicUsize::new(0);
pub static DISPATCH_CALLS: AtomicUsize = AtomicUsize::new(0);

/// One render-trace record, serialized to NDJSON; the `ev` tag names the event. Pipeline-hook
/// variants omit `eye` -- it's injected by [`trace_eye`]; the frame/eye markers carry it directly.
#[derive(serde::Serialize)]
#[serde(tag = "ev")]
pub enum TraceEvent {
    #[serde(rename = "frame_begin")]
    FrameBegin {
        stereo: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        present_eye: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        restore_counters: Option<bool>,
    },
    #[serde(rename = "draw_begin")]
    DrawBegin { eye: usize },
    #[serde(rename = "draw_end")]
    DrawEnd {
        eye: usize,
        draw: usize,
        draw_indexed: usize,
        dispatch: usize,
    },
    #[serde(rename = "SetupRenderCamera")]
    SetupRenderCamera,
    #[serde(rename = "SetupRenderFrameData")]
    SetupRenderFrameData { gated: bool },
    #[serde(rename = "HandBackBuffers")]
    HandBackBuffers { gated: bool },
    #[serde(rename = "SmoothedExposureUpdate")]
    SmoothedExposureUpdate { gated: bool, exposure: f32 },
    #[serde(rename = "CalcHistogramMidBright")]
    CalcHistogramMidBright { gated: bool },
    #[serde(rename = "GenerateHistogram")]
    GenerateHistogram { skip: bool },
    #[serde(rename = "DoF::Apply")]
    DofApply { input: u32, skip: bool },
    #[serde(rename = "MotionBlur::Apply")]
    MotionBlurApply { input: u32, skip: bool },
    #[serde(rename = "Glare::Apply")]
    GlareApply { skip: bool },
    #[serde(rename = "Fade::Apply")]
    FadeApply { skip: bool },
    #[serde(rename = "PlayerDamage::Apply")]
    PlayerDamageApply { input: u32, skip: bool },
    #[serde(rename = "SunHalo::PreApply")]
    SunHaloPreApply { skip: bool },
    #[serde(rename = "SunHalo::Apply")]
    SunHaloApply { skip: bool },
    #[serde(rename = "PostDraw")]
    PostDraw,
    #[serde(rename = "Flip")]
    Flip { blocked: bool },
    // Buffer-flow events (raw pointers as u64 so render-setup / texture instances can be compared
    // across eyes -- same pointer = same target, different pointer = a swapped instance).
    #[serde(rename = "SetRenderSetup")]
    SetRenderSetup {
        setup: u64,
        /// Draws/dispatches issued into the *previous* target since the last bind (this thread).
        draws: usize,
        indexed: usize,
        dispatch: usize,
    },
    #[serde(rename = "Clear")]
    Clear { color: [f32; 4] },
    #[serde(rename = "CopySurfaceToTexture")]
    CopySurfaceToTexture { dst: u64, src: u64 },
    #[serde(rename = "ResolveSurface")]
    ResolveSurface,
}

/// Append one trace record (frame/eye markers, which carry their own `eye` field), while active.
pub fn trace(event: TraceEvent) {
    if TRACE_FRAMES.load(Ordering::Relaxed) > 0
        && let Ok(s) = serde_json::to_string(&event)
    {
        TRACE_LOG.lock().push(s);
    }
}

/// Append one trace record from inside a per-dispatch pipeline hook, injecting the current eye.
pub fn trace_eye(event: TraceEvent) {
    if TRACE_FRAMES.load(Ordering::Relaxed) > 0
        && let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&event)
    {
        map.insert("eye".to_string(), DRAW_INDEX.load(Ordering::Relaxed).into());
        TRACE_LOG
            .lock()
            .push(serde_json::Value::Object(map).to_string());
    }
}

/// Begin a render-call trace covering the next `frames` real frames.
pub fn trace_start(frames: i32) {
    TRACE_LOG.lock().clear();
    TRACE_FRAMES.store(frames, Ordering::Relaxed);
    tracing::info!("Render trace started ({frames} frames)");
}

/// Called once per real frame by the Draw driver; decrements the trace counter and, when the run
/// finishes, writes the collected trace as NDJSON next to the injected DLL (same place as
/// `jc3vrs.log`), recording the absolute path for the UI.
pub fn trace_end_frame() {
    if TRACE_FRAMES.load(Ordering::Relaxed) <= 0 {
        return;
    }
    if TRACE_FRAMES.fetch_sub(1, Ordering::Relaxed) - 1 <= 0 {
        let log = TRACE_LOG.lock();
        let path = module::get_path()
            .and_then(|p| p.parent().map(|dir| dir.join("jc3vrs_render_trace.ndjson")))
            .unwrap_or_else(|| std::path::PathBuf::from("jc3vrs_render_trace.ndjson"));
        match std::fs::write(&path, log.join("\n")) {
            Ok(()) => {
                let shown = path.display().to_string();
                tracing::info!("Render trace dumped: {} records -> {}", log.len(), shown);
                *LAST_TRACE_PATH.lock() = Some(shown);
            }
            Err(e) => tracing::error!("Failed to write render trace: {e}"),
        }
    }
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _unk: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module).ok();
            module::set(module);
        };
    }
    true
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn run(_: *mut c_void) {
    initialize_startup();
}

/// Called when the DLL is loaded
fn initialize_startup() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();

        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::capture();

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = tracing::field::display(backtrace),
            "A panic occurred",
        );
    }));

    logging::install();
    tracing::info!("JC3VRS startup");
    crash::install();
    hooks::install();
}

/// Called to undo `initialize_startup` and eject
fn shutdown_startup() {
    tracing::info!("Uninstalling hooks");
    hooks::uninstall();

    // Wait to ensure we're clear of the blast radius of the hooks
    std::thread::sleep(std::time::Duration::from_millis(100));

    tracing::info!("Ejecting");
    logging::uninstall();
    module::exit();
}

/// Called when we're on the game thread for the first time
fn initialize_from_game() -> anyhow::Result<()> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if INITIALIZED.get().is_some() {
        return Ok(());
    }
    INITIALIZED.set(true).unwrap();

    EguiState::install()?;
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    if let Some(egui_state) = EguiState::get().as_mut() {
        let mut state = EGUI_DEBUG_RENDER_STATE.lock();
        state.uninstall(&mut egui_state.egui_renderer);
    }
    EguiState::uninstall();
}

/// Request that we shut down and exit
fn shutdown() {
    static SHUTDOWN: OnceLock<bool> = OnceLock::new();
    if SHUTDOWN.get().is_some() {
        return;
    }
    SHUTDOWN.set(true).unwrap();

    tracing::info!("Shutting down");
    shutdown_from_game();
    std::thread::spawn(shutdown_startup);
}

fn update() {
    if let Err(e) = initialize_from_game() {
        tracing::error!("Failed to initialize in game thread, shutting down: {e:?}");
        shutdown();
        return;
    }

    let panic = std::panic::catch_unwind(|| {
        if util::is_pressed(VK_F5) {
            shutdown();
            return;
        }

        if let Some(egui_state) = EguiState::get().as_mut() {
            if util::is_pressed(VK_F6) {
                egui_state.toggle_game_input_capture();
            }

            egui_state.run(|ctx, renderer| {
                egui::Window::new("Debug").show(ctx, |ui| egui_debug_window(ui, renderer));
            });
        }
    });
    if let Err(e) = panic {
        let panic_msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
        tracing::error!("Panic in update, shutting down: {panic_msg:?}");
        shutdown();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EguiTab {
    Render,
    Camera,
    Game,
}
impl std::fmt::Display for EguiTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
static EGUI_TAB: Mutex<EguiTab> = Mutex::new(EguiTab::Render);
impl EguiTab {
    fn ui(ui: &mut egui::Ui) {
        let mut tab = EGUI_TAB.lock();
        ui.horizontal(|ui| {
            for candidate_tab in [EguiTab::Render, EguiTab::Camera, EguiTab::Game] {
                if ui
                    .add(
                        egui::Button::new(candidate_tab.to_string())
                            .selected(*tab == candidate_tab),
                    )
                    .clicked()
                {
                    *tab = candidate_tab;
                }
            }
        });
    }

    fn get() -> EguiTab {
        *EGUI_TAB.lock()
    }
}

pub fn egui_debug_window(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    EguiTab::ui(ui);

    match EguiTab::get() {
        EguiTab::Render => {
            egui_debug_render(ui, renderer);
        }
        EguiTab::Camera => {
            egui_debug_camera(ui);
        }
        EguiTab::Game => {
            egui_debug_game(ui);
        }
    }
}

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

struct EguiDebugRenderState {
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
static EGUI_DEBUG_RENDER_STATE: Mutex<EguiDebugRenderState> =
    Mutex::new(EguiDebugRenderState::new());

/// Preview thumbnail width (px) in the Render tab; user-controllable via a slider.
static PREVIEW_WIDTH: Mutex<f32> = Mutex::new(176.0);

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

/// Snapshot of the render camera's projection state, captured after each eye's Draw so the two
/// eyes can be compared in the debug UI (to isolate the eye-1 projection corruption).
#[derive(Copy, Clone)]
pub struct CameraSnapshot {
    pub valid: bool,
    pub camera_ptr: usize,
    pub state_bits: u8,
    pub offcenter_tiles: i32,
    pub offcenter_tile_x: i32,
    pub offcenter_tile_y: i32,
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub width: i32,
    pub height: i32,
    pub projection: [f32; 16],
    pub view: [f32; 16],
    pub view_proj_f: [f32; 16],
    pub transform: [f32; 16],
}
impl CameraSnapshot {
    const fn empty() -> Self {
        Self {
            valid: false,
            camera_ptr: 0,
            state_bits: 0,
            offcenter_tiles: 0,
            offcenter_tile_x: 0,
            offcenter_tile_y: 0,
            fov: 0.0,
            near: 0.0,
            far: 0.0,
            aspect: 0.0,
            width: 0,
            height: 0,
            projection: [0.0; 16],
            view: [0.0; 16],
            view_proj_f: [0.0; 16],
            transform: [0.0; 16],
        }
    }
}

/// Per-eye render-camera snapshots (index 0 / 1), filled by [`capture_render_camera`].
pub static CAMERA_SNAPSHOTS: Mutex<[CameraSnapshot; 2]> =
    Mutex::new([CameraSnapshot::empty(), CameraSnapshot::empty()]);

/// Snapshot `CameraManager::m_RenderCamera` into slot `index`. Call after the eye's Draw has been
/// drained, so the captured projection is the one that eye actually rendered with.
pub fn capture_render_camera(index: usize) {
    unsafe {
        let Some(cm) = jc3gi::camera::camera_manager::CameraManager::get() else {
            return;
        };
        let Some(cam) = cm.m_RenderCamera.as_ref() else {
            return;
        };
        let snap = CameraSnapshot {
            valid: true,
            camera_ptr: cm.m_RenderCamera as usize,
            state_bits: cam.m_StateBitfield.bits(),
            offcenter_tiles: cam.m_OffCenterTiles,
            offcenter_tile_x: cam.m_OffCenterTileX,
            offcenter_tile_y: cam.m_OffCenterTileY,
            fov: cam.m_FOV,
            near: cam.m_Near,
            far: cam.m_Far,
            aspect: cam.m_AspectRatio,
            width: cam.m_Width,
            height: cam.m_Height,
            projection: cam.m_Projection.data,
            view: cam.m_View.data,
            view_proj_f: cam.m_ViewProjectionF.data,
            transform: cam.m_TransformF.data,
        };
        if let Some(slot) = CAMERA_SNAPSHOTS.lock().get_mut(index) {
            *slot = snap;
        }
    }
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

fn egui_debug_render(ui: &mut egui::Ui, renderer: &mut egui_directx11::Renderer) {
    {
        let mut stereo = STEREO.load(Ordering::Relaxed);
        if ui
            .checkbox(&mut stereo, "Stereo (double-Draw)")
            .on_hover_text(
                "Issue a second game.Draw per frame; CClock::Update is gated to once/frame",
            )
            .changed()
        {
            STEREO.store(stereo, Ordering::Relaxed);
        }

        {
            let mut sc = STEREO_CAMERAS.load(Ordering::Relaxed);
            if ui
                .checkbox(&mut sc, "Stereo cameras (per-eye IPD offset)")
                .on_hover_text("Offset the active camera per eye so the two draws diverge")
                .changed()
            {
                STEREO_CAMERAS.store(sc, Ordering::Relaxed);
            }
            ui.add(egui::Slider::new(&mut *STEREO_IPD.lock(), 0.0..=100.0).text("IPD (m)"));
        }

        {
            let calls = hooks::camera::SETUP_RC_CALLS.load(Ordering::Relaxed);
            let hits = hooks::camera::SETUP_RC_HITS.load(Ordering::Relaxed);
            let expected = unsafe {
                jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()
                    .map(|ge| ge as *mut _ as usize + 0x170)
                    .unwrap_or(0)
            };
            ui.label(format!(
                "SetupRC: calls={calls} hits={hits}  expected render-cam={expected:#x}"
            ));
        }

        gate_checkbox(
            ui,
            &hooks::game::RESTORE_FRAME_COUNTERS,
            "Restore frame counters between eyes (fixes jitter/parity flicker)",
        );
        gate_checkbox(
            ui,
            &PRESENT_EYE_0,
            "Present eye 0 (else eye 1) -- flip to compare each eye live",
        );
        ui.horizontal(|ui| {
            if ui.button("Dump render trace (4 frames)").clicked() {
                trace_start(4);
            }
            let remaining = TRACE_FRAMES.load(Ordering::Relaxed);
            if remaining > 0 {
                ui.label(format!("tracing... {remaining} frames left"));
            } else if LAST_TRACE_PATH.lock().is_some() {
                ui.label("dumped");
            } else {
                ui.label("(writes next to the DLL)");
            }
        });

        ui.collapsing("Eye-1 gates (skip on second Draw)", |ui| {
            use hooks::stereo::{
                GATE_EXPOSURE, GATE_HAND_BACK_BUFFERS, GATE_SETUP_RENDER_FRAME_DATA,
            };
            gate_checkbox(
                ui,
                &GATE_EXPOSURE,
                "Auto-exposure (SmoothedExposure + Histogram)",
            );
            gate_checkbox(
                ui,
                &GATE_SETUP_RENDER_FRAME_DATA,
                "SetupRenderFrameData (RBI list swap)",
            );
            gate_checkbox(
                ui,
                &GATE_HAND_BACK_BUFFERS,
                "HandBackBuffers (constant-buffer recycle)",
            );
        });

        ui.collapsing("Post-FX (reprojection passes, both eyes)", |ui| {
            use hooks::post_effects::{
                DOF_NO_REPROJECT, SKIP_DOF, SKIP_MOTION_BLUR, SKIP_MOTION_BLUR_RECON,
            };
            gate_checkbox(ui, &SKIP_MOTION_BLUR, "Skip MotionBlur::Apply (whole pass)");
            gate_checkbox(
                ui,
                &SKIP_MOTION_BLUR_RECON,
                "Skip MotionBlur recon (if pass not skipped)",
            );
            gate_checkbox(
                ui,
                &DOF_NO_REPROJECT,
                "DoF: plain composite, no reprojection (keeps picture)",
            );
            gate_checkbox(ui, &SKIP_DOF, "Skip DepthOfField::Apply (washes out!)");
        });

        ui.collapsing("Post-FX stages (skip to bisect)", |ui| {
            use hooks::post_effects::{
                SKIP_FADE, SKIP_GLARE, SKIP_HISTOGRAM, SKIP_PLAYER_DAMAGE, SKIP_SUN_HALO,
            };
            gate_checkbox(
                ui,
                &SKIP_HISTOGRAM,
                "Exposure histogram (stalls auto-exposure)",
            );
            gate_checkbox(ui, &SKIP_GLARE, "Glare / bloom");
            gate_checkbox(ui, &SKIP_FADE, "Fade");
            gate_checkbox(ui, &SKIP_SUN_HALO, "Sun halo");
            gate_checkbox(ui, &SKIP_PLAYER_DAMAGE, "Player-damage vignette");
        });
    }

    let preview_width = {
        let mut w = PREVIEW_WIDTH.lock();
        ui.add(egui::Slider::new(&mut *w, 48.0..=512.0).text("Preview size (px)"));
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

            ui.label("Per-eye pipeline (rows = eyes, columns = stages, left to right):");
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

            ui.separator();
            ui.label("Render targets (live; eye 1 is the last-rendered pass):");
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

            ui.separator();
            ui.label("Per-eye render camera (captured after each Draw; differences in yellow):");
            let (s0, s1) = {
                let g = CAMERA_SNAPSHOTS.lock();
                (g[0], g[1])
            };
            ui.columns(2, |cols| {
                show_camera_snapshot(&mut cols[0], "Eye 0", &s0, &s1);
                show_camera_snapshot(&mut cols[1], "Eye 1", &s1, &s0);
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

fn matrix_grid(ui: &mut egui::Ui, id: &str, label: &str, m: &[f32; 16], other: Option<&[f32; 16]>) {
    ui.label(label);
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for r in 0..4 {
            for c in 0..4 {
                let i = r * 4 + c;
                let v = m[i];
                let differs = other.is_some_and(|o| (v - o[i]).abs() > 1e-5);
                let text = format!("{v:+.3}");
                if differs {
                    ui.colored_label(egui::Color32::YELLOW, text);
                } else {
                    ui.label(text);
                }
            }
            ui.end_row();
        }
    });
}

fn egui_debug_camera(ui: &mut egui::Ui) {
    let mut cs = hooks::camera::CAMERA_SETTINGS.lock();
    ui.checkbox(&mut cs.enabled, "Enabled");
    ui.checkbox(&mut cs.always_use_t1, "Always use T1");
    ui.checkbox(&mut cs.blurs_enabled, "Blurs");
    ui.checkbox(&mut cs.use_eye_matrices, "Use eye matrices");

    ui.add_enabled_ui(!cs.use_eye_matrices, |ui| {
        use egui::Slider;
        ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text("Head X"));
        ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text("Head Y"));
        ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text("Head Z"));

        ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
        ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
        ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));
    });
}

fn egui_debug_game(ui: &mut egui::Ui) {
    unsafe {
        let Some(game) = jc3gi::game::Game::get() else {
            return;
        };
        let Some(clock) = jc3gi::clock::Clock::get() else {
            return;
        };

        ui.heading("Game");
        ui.label(format!("Update frequency: {}Hz", game.m_UpdateFrequency));
        ui.label(format!("Update flags: {:X}", game.m_UpdateFlags));
        ui.label(format!(
            "Interpolation method: {:X}",
            game.m_InterpolationMethod
        ));
        {
            let mut interpolation_override = game.m_InterpolationOverride;
            let before = interpolation_override;
            egui::ComboBox::from_label("Interpolation override")
                .selected_text(interpolation_override.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut interpolation_override, -1, "Really None");
                    ui.selectable_value(&mut interpolation_override, 0, "None");
                    ui.selectable_value(&mut interpolation_override, 1, "1");
                    ui.selectable_value(&mut interpolation_override, 2, "2");
                    ui.selectable_value(&mut interpolation_override, 3, "3");
                });

            if before != interpolation_override
                && let Some(mut patcher) = hooks::patcher()
            {
                patcher.patch(
                    &mut game.m_InterpolationOverride as *mut _ as usize,
                    &interpolation_override.to_le_bytes(),
                );
            }
        }
        patchbox(ui, "Decouple enabled", &mut game.m_DecoupleEnabled);

        ui.heading("Clock");
        ui.label(format!("FPS: {}", clock.m_FPS));
        ui.label(format!("SPF: {}", clock.m_SPF));
        ui.label(format!("Real FPS: {}", clock.m_RealFPS));
        ui.label(format!("Real SPF: {}", clock.m_RealSPF));
        ui.label(format!("Update speed: {}", clock.m_UpdateSpeed));
        ui.label(format!("Force to FPS: {}", clock.m_ForceToThisFPS));
        ui.label(format!("Force to SPF: {}", clock.m_ForceToThisSPF));
        patchbox(ui, "Stop", &mut clock.m_Stop);
        patchbox(ui, "Force to FPS", &mut clock.m_ForceToFps);
    }
}

fn patchbox(ui: &mut egui::Ui, label: &str, value: *mut bool) {
    let mut enabled = unsafe { *value };
    if ui.checkbox(&mut enabled, label).changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &[if enabled { 1 } else { 0 }]);
        }
    }
}
