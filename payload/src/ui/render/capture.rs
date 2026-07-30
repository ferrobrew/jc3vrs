//! The render-thread capture state behind the Render and Previews tabs: the per-eye back-buffer
//! captures the VR presentation path submits, the HDR scene and post-effect stage snapshots the
//! Previews tab displays, and the visibility gate that keeps those preview-only copies from running
//! when nobody is looking.

use std::time::{Duration, Instant};

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

/// Post-stage indices (matching the Previews tab's stage labels); used by the stage detours.
pub const POST_STAGE_DOF: usize = 0;
pub const POST_STAGE_MB: usize = 1;

/// A per-eye snapshot of one post-effect stage's result texture. The debug texture + SRV are
/// created on the render thread (where the stage runs); the egui id is registered lazily on the UI
/// thread.
#[derive(Default)]
pub(in crate::ui) struct StageCapture {
    created_desc: Option<(u32, u32, i32)>,
    texture: Option<ID3D11Texture2D>,
    pub(in crate::ui) srv: Option<ID3D11ShaderResourceView>,
    pub(in crate::ui) egui_id: Option<egui::TextureId>,
}

pub struct EguiDebugRenderState {
    /// Final back-buffer capture per Draw (eye): index 0 and index 1.
    /// The per-eye captures the VR presentation path submits. The egui id is `None` until the UI
    /// thread registers one for the Previews panel: these textures must exist whether or not the
    /// debug UI ever installs, so their creation cannot depend on a renderer.
    pub(in crate::ui) target_textures: [Option<(ID3D11Texture2D, Option<egui::TextureId>)>; 2],
    /// HDR scene (MainColor, pre-post) capture per eye -- the first column of the pipeline rows.
    pub(in crate::ui) main_color_textures: [Option<(ID3D11Texture2D, egui::TextureId)>; 2],
    /// (w, h) the back-buffer captures were built for; recreate them when the back buffer resizes.
    target_size: Option<(u32, u32)>,
    /// (w, h, dxgi format) the MainColor captures were built for; recreate on change.
    main_color_desc: Option<(u32, u32, i32)>,
    /// Per-(stage, eye) captures of intermediate post-effect results: index `stage * 2 + eye`.
    pub(in crate::ui) post_stage_captures: Vec<StageCapture>,
    /// Cache of engine SRV pointer -> (width, height, egui texture id), for the live render-target
    /// thumbnails. The size rides along so a reused SRV address backed by a resized surface (see
    /// [`Self::thumbnail_id`]) is detected instead of serving the stale image.
    srv_thumbnails: Vec<(usize, (u32, u32), egui::TextureId)>,
    /// Egui texture ids dropped on the render thread (a post-stage capture's descriptor changed)
    /// that still need `unregister_user_texture`, which only the UI thread can reach a renderer
    /// for. Drained by [`Self::prepare_if_necessary`].
    pending_unregisters: Vec<egui::TextureId>,
    /// Whether the Previews tab counted as visible ([`previews_visible`]) as of the last
    /// `prepare_if_necessary`, so a visible -> not-visible transition is caught exactly once to
    /// free the preview-only captures.
    previews_were_visible: bool,
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
            pending_unregisters: Vec::new(),
            previews_were_visible: false,
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
        // The renderer that owns the egui registration is only reachable from the UI thread, so a
        // dropped id from here (the render thread) would leak the registration and the
        // full-resolution SRV it pins. Queue it for `prepare_if_necessary` to unregister once it
        // next runs with a renderer, instead of just discarding it.
        if self.post_stage_captures[idx].created_desc != Some(desc)
            && let Some(id) = self.post_stage_captures[idx].egui_id.take()
        {
            self.pending_unregisters.push(id);
        }
        let cap = &mut self.post_stage_captures[idx];
        unsafe {
            if cap.created_desc != Some(desc) {
                cap.texture = None;
                cap.srv = None;
                let mut texture: Option<ID3D11Texture2D> = None;
                if let Err(e) = device.m_Device.CreateTexture2D(
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
                ) {
                    // Stamp `created_desc` even on failure so this does not retry every stage of
                    // every frame forever with only "(no capture)" visible to the user.
                    tracing::error!(
                        "failed to create the post-stage capture texture for stage {stage}, eye \
                         {eye}: {e:?}"
                    );
                    cap.created_desc = Some(desc);
                    return;
                }
                let Some(texture) = texture else {
                    cap.created_desc = Some(desc);
                    return;
                };
                let mut srv: Option<ID3D11ShaderResourceView> = None;
                if let Err(e) =
                    device
                        .m_Device
                        .CreateShaderResourceView(&texture, None, Some(&mut srv))
                {
                    tracing::error!(
                        "failed to create the post-stage capture SRV for stage {stage}, eye \
                         {eye}: {e:?}"
                    );
                    cap.created_desc = Some(desc);
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

    /// Get (registering+caching on first use) an egui texture id for an engine SRV. Keyed on the
    /// SRV's raw pointer *and* its current size, so a reused address backed by a resized surface
    /// (a resolution change recreates the engine's render targets) is treated as a fresh SRV
    /// instead of returning a retained id that shows the pre-resize image.
    pub(in crate::ui) fn thumbnail_id(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
        srv_raw: usize,
        srv: &ID3D11ShaderResourceView,
    ) -> egui::TextureId {
        let size = unsafe { srv_texture_size(srv) };
        if let Some(pos) = self
            .srv_thumbnails
            .iter()
            .position(|(p, _, _)| *p == srv_raw)
        {
            let (_, cached_size, id) = self.srv_thumbnails[pos];
            if cached_size == size {
                return id;
            }
            renderer.unregister_user_texture(id);
            self.srv_thumbnails.remove(pos);
        }
        let id = renderer.register_user_texture(srv.clone());
        self.srv_thumbnails.push((srv_raw, size, id));
        id
    }

    pub(crate) fn prepare_if_necessary(&mut self, renderer: &mut egui_directx11::Renderer) {
        // Drain post-stage egui ids the render thread dropped without a renderer to unregister
        // them with (see `capture_post_stage`); this is the first point back on the UI thread.
        for id in self.pending_unregisters.drain(..) {
            renderer.unregister_user_texture(id);
        }

        unsafe {
            let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
                return;
            };
            let Some(device) = ge.m_Device.as_mut() else {
                return;
            };

            // Register egui ids for any eye capture that does not have one yet, so the Previews
            // panel can display textures `ensure_eye_targets` created without a renderer.
            for slot in self.target_textures.iter_mut().flatten() {
                if slot.1.is_none()
                    && let Some(srv) = Self::view_for(device, &slot.0)
                {
                    slot.1 = Some(renderer.register_user_texture(srv));
                }
            }

            // The MainColor and post-stage captures exist only for the Previews tab, so they track
            // its visibility: created/refreshed while it is open, released the first frame it is
            // not, so an occasional peek does not pin full-resolution surfaces for the rest of the
            // session.
            let visible = previews_visible();
            if visible {
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
                            *slot =
                                Self::create_target(device, desc.0, desc.1, DXGI_FORMAT(desc.2))
                                    .map(|(texture, srv)| {
                                        (texture, renderer.register_user_texture(srv))
                                    });
                        }
                        self.main_color_desc = Some(desc);
                    }
                }
            } else if self.previews_were_visible {
                self.free_preview_captures(renderer);
            }
            self.previews_were_visible = visible;
        }
    }

    /// Release the MainColor and post-stage captures and their egui registrations. Called once,
    /// when the Previews tab stops being visible -- these captures have no consumer besides that
    /// tab (unlike `target_textures`, which the VR presentation path reads).
    fn free_preview_captures(&mut self, renderer: &mut egui_directx11::Renderer) {
        for slot in &mut self.main_color_textures {
            if let Some((_, id)) = slot.take() {
                renderer.unregister_user_texture(id);
            }
        }
        self.main_color_desc = None;
        for cap in &mut self.post_stage_captures {
            cap.texture = None;
            cap.srv = None;
            cap.created_desc = None;
            if let Some(id) = cap.egui_id.take() {
                renderer.unregister_user_texture(id);
            }
        }
    }

    /// Create or resize the two per-eye captures the VR presentation path submits.
    ///
    /// Deliberately takes no renderer and is driven from the frame loop rather than from inside the
    /// egui closure. These textures are what VR presents; they existed only as a side effect of the
    /// debug UI preparing itself, so an `EguiState` that failed to install left VR with nothing to
    /// submit and no indication why.
    pub(crate) fn ensure_eye_targets(&mut self) {
        // SAFETY: reads the live engine device on the game thread, as the UI preparation does.
        unsafe {
            let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
                return;
            };
            let Some(device) = ge.m_Device.as_mut() else {
                return;
            };
            // Under single-pass double-wide the render target holds both eye-halves side by side, so
            // each capture is half its width -- the collapse's split copies one full-width half in.
            let Some(size) = crate::stereo::per_eye_render_size() else {
                return;
            };
            if self.target_size == Some(size) && self.target_textures.iter().all(Option::is_some) {
                return;
            }
            for slot in &mut self.target_textures {
                if let Some((_, Some(id))) = slot.take() {
                    // The renderer is not reachable here; the UI thread drains this queue.
                    self.pending_unregisters.push(id);
                }
                *slot = Self::create_target(device, size.0, size.1, DXGI_FORMAT_R8G8B8A8_UNORM)
                    .map(|(texture, _)| (texture, None));
            }
            self.target_size = Some(size);
        }
    }

    /// A fresh shader-resource view over `texture`, for a late egui registration.
    fn view_for(
        device: &jc3gi::graphics_engine::device::Device,
        texture: &ID3D11Texture2D,
    ) -> Option<ID3D11ShaderResourceView> {
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: `texture` was created on this device with `D3D11_BIND_SHADER_RESOURCE`.
        unsafe {
            device
                .m_Device
                .CreateShaderResourceView(texture, None, Some(&mut srv))
                .ok()?;
        }
        srv
    }

    fn create_target(
        device: &jc3gi::graphics_engine::device::Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Option<(ID3D11Texture2D, ID3D11ShaderResourceView)> {
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

            Some((texture, srv))
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

    /// Tear down the captured D3D surfaces and, where a renderer is reachable, their egui
    /// registrations too. The D3D side is released unconditionally: an `EguiState` that failed to
    /// install still holds the captures from before the failure, and without a renderer to
    /// unregister with, dropping them is the only cleanup possible -- but it must still happen, or
    /// every failed inject/eject cycle leaks the full-resolution captured surfaces.
    fn uninstall(&mut self, renderer: Option<&mut egui_directx11::Renderer>) {
        match renderer {
            Some(renderer) => {
                // The eye captures carry an optional id: one created before the UI installed has
                // never been registered, so there is nothing to unregister for it.
                for slot in &mut self.target_textures {
                    if let Some((_, Some(texture_id))) = slot.take() {
                        renderer.unregister_user_texture(texture_id);
                    }
                }
                for slot in &mut self.main_color_textures {
                    if let Some((_, texture_id)) = slot.take() {
                        renderer.unregister_user_texture(texture_id);
                    }
                }
                for (_, _, texture_id) in self.srv_thumbnails.drain(..) {
                    renderer.unregister_user_texture(texture_id);
                }
                for cap in self.post_stage_captures.drain(..) {
                    if let Some(id) = cap.egui_id {
                        renderer.unregister_user_texture(id);
                    }
                }
                for id in self.pending_unregisters.drain(..) {
                    renderer.unregister_user_texture(id);
                }
            }
            None => {
                self.target_textures = [None, None];
                self.main_color_textures = [None, None];
                self.srv_thumbnails.clear();
                self.post_stage_captures.clear();
                self.pending_unregisters.clear();
            }
        }
    }
}

/// The width/height of an SRV's underlying `ID3D11Texture2D`, used by
/// [`EguiDebugRenderState::thumbnail_id`] to detect a resized surface that reused its predecessor's
/// SRV address.
unsafe fn srv_texture_size(srv: &ID3D11ShaderResourceView) -> (u32, u32) {
    unsafe {
        let Ok(resource) = srv.GetResource() else {
            return (0, 0);
        };
        let Ok(texture) = resource.cast::<ID3D11Texture2D>() else {
            return (0, 0);
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        (desc.Width, desc.Height)
    }
}

pub static EGUI_DEBUG_RENDER_STATE: Mutex<EguiDebugRenderState> =
    Mutex::new(EguiDebugRenderState::new());

/// How long after [`mark_previews_visible`] was last called the Previews tab still counts as open.
/// The tab has no "closed" event to hook -- an `egui_dock` tab simply stops being drawn once it is
/// not the active tab in its leaf -- so recency stands in for it: a few frames' slack absorbs
/// frame-to-frame jitter while still reclaiming the preview-only captures promptly once the tab
/// stops drawing.
const PREVIEWS_VISIBLE_TIMEOUT: Duration = Duration::from_millis(250);

/// When the Previews tab last drew, per [`mark_previews_visible`]; `None` before it ever has.
static PREVIEWS_LAST_SEEN: Mutex<Option<Instant>> = Mutex::new(None);

/// Mark the Previews tab visible for the current frame. The debug-only capture path
/// (`capture_post_stage`, `capture_main_color`) gates on [`previews_visible`], so nobody pays for a
/// full-size `CopyResource` per stage per eye for a panel almost nobody has open -- call this once
/// per frame from the top of the tab's body.
pub fn mark_previews_visible() {
    *PREVIEWS_LAST_SEEN.lock() = Some(Instant::now());
}

/// Whether the Previews tab has drawn recently enough to still count as open (see
/// [`PREVIEWS_VISIBLE_TIMEOUT`]). Read by the post-stage and MainColor capture call sites to skip
/// their `CopyResource`, and by [`EguiDebugRenderState::prepare_if_necessary`] to know when to
/// release the captures it has been maintaining.
pub fn previews_visible() -> bool {
    PREVIEWS_LAST_SEEN
        .lock()
        .is_some_and(|t| t.elapsed() < PREVIEWS_VISIBLE_TIMEOUT)
}

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
