//! The far-field share (issue #32, increment 2): render the far-regime scene once per frame and
//! composite it under both eyes.
//!
//! A share frame runs three dispatches (`hooks::game`): a far-only dispatch at eye 0's pose
//! (far model runs + the gated far-regime types, G-buffer range only), whose G-buffer and depth
//! are captured here; then the two per-eye near dispatches, each of which composites the captured
//! far G-buffer back in — after the engine's clears and Z prepass, before the geometry passes — so
//! the stock deferred lighting then resolves a *complete* G-buffer per eye, bit-identical to a
//! full render. Sharing the G-buffer (rather than a lit far image) sidesteps the lighting-resolve
//! masking problem entirely; the cost is that lighting stays per-eye. See `docs/mod/far-field.md`.
//!
//! Eye 0's composite is an identity mapping (the far dispatch used its pose and projection); eye
//! 1's maps through the per-axis affine NDC reprojection between the two off-axis projections
//! (exact for parallel same-centre eyes; the IPD translation is the threshold-bounded parallax
//! residual). The depth merge with the near dispatch's own Z prepass is fixed-function: the
//! composite draws with `GREATER_EQUAL` + depth write under reverse-Z.

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use parking_lot::Mutex;
use windows::Win32::{
    Graphics::{
        Direct3D11::{
            D3D11_BIND_SHADER_RESOURCE, D3D11_BLEND_DESC, D3D11_BUFFER_DESC,
            D3D11_COMPARISON_GREATER_EQUAL, D3D11_CPU_ACCESS_WRITE, D3D11_CULL_NONE,
            D3D11_DEPTH_STENCIL_DESC, D3D11_DEPTH_WRITE_MASK_ALL, D3D11_FILL_SOLID,
            D3D11_MAP_WRITE_DISCARD, D3D11_RASTERIZER_DESC, D3D11_SUBRESOURCE_DATA,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT,
            ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11DeviceContext,
            ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
            ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
        },
        Dxgi::Common::{
            DXGI_FORMAT, DXGI_FORMAT_R24_UNORM_X8_TYPELESS, DXGI_FORMAT_R24G8_TYPELESS,
            DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS, DXGI_FORMAT_R32G8X24_TYPELESS, DXGI_SAMPLE_DESC,
        },
    },
    System::Threading::{EnterCriticalSection, LeaveCriticalSection},
};

/// Capture the far dispatch's G-buffer: copy MainDepth + GBuffer0..3 into the share pipeline's own
/// textures (recreated on size/format change). Called from the render-pass-range hook after the
/// far dispatch's G-buffer range completes, on the render thread.
pub fn capture_far_gbuffer() {
    if let Err(e) = with_context(|device, context| {
        let mut guard = SHARE.lock();
        if guard.is_none() {
            *guard = Some(SharePipeline::new(device)?);
        }
        let share = guard.as_mut().expect("the share pipeline was just ensured");
        // SAFETY: `context` is the live engine immediate context, held under its mutex.
        unsafe { share.capture(device, context) }
    }) {
        tracing::warn!(target: "far_field", "far G-buffer capture failed: {e:#}");
    }
}

/// Composite the captured far G-buffer into the current (near) dispatch's targets for `eye`.
/// Called from the render-pass-range hook between the clear/Z-prepass passes and the geometry
/// passes, on the render thread. A no-op (with a warning) when no capture exists yet.
pub fn composite(eye: usize) {
    if let Err(e) = with_context(|device, context| {
        let mut guard = SHARE.lock();
        if guard.is_none() {
            *guard = Some(SharePipeline::new(device)?);
        }
        let share = guard.as_mut().expect("the share pipeline was just ensured");
        // SAFETY: `context` is the live engine immediate context, held under its mutex.
        unsafe { share.composite(context, eye) }
    }) {
        tracing::warn!(target: "far_field", "far-field composite failed (eye {eye}): {e:#}");
    }
}

/// Tear down the share pipeline (COM release), dropping any captured frame.
pub fn teardown() {
    *SHARE.lock() = None;
}

/// The committed, precompiled shaders: the shared fullscreen-triangle vertex shader and the
/// G-buffer composite pixel shader.
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const COMPOSITE_DXBC: &[u8] = include_bytes!("../shaders/far_field_composite_ps.dxbc");

/// The share pipeline singleton, built lazily on first use. Holds COM objects, which `windows`
/// marks `Send`/`Sync`, so a `Mutex` static is sound.
static SHARE: Mutex<Option<SharePipeline>> = Mutex::new(None);

/// Acquire the engine device and immediate context, run `body` under the context mutex. Mirrors
/// the foveation/blit context discipline.
fn with_context<T>(
    body: impl FnOnce(&Device, &ID3D11DeviceContext) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("far field: the graphics engine is unavailable")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("far field: the graphics device is unavailable")?;
    let context = unsafe { device.m_Context.as_ref() }
        .context("far field: the graphics context is unavailable")?;
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        let result = body(device, &context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
        result
    }
}

/// One captured target: the copy texture, its SRV, and the descriptor it was built for.
struct Capture {
    texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    desc: (u32, u32, i32),
}

/// The share pipeline: shaders, the depth-merge depth-stencil state, and the five captured
/// far-dispatch targets (MainDepth + GBuffer0..3).
struct SharePipeline {
    vertex_shader: ID3D11VertexShader,
    composite_shader: ID3D11PixelShader,
    /// `GREATER_EQUAL` + depth write, stencil off: the reverse-Z merge with the near Z prepass.
    depth_merge_state: ID3D11DepthStencilState,
    /// Opaque, write-all-channels blending: the G-buffer channels are data, and inheriting an
    /// alpha-blending state from the surrounding passes dims every composited value against the
    /// cleared background.
    opaque_blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    /// The `CompositeParams` constant buffer (`b0`): the per-eye NDC affine.
    params_cb: ID3D11Buffer,
    /// GBuffer0..3 captures.
    gbuffer: [Option<Capture>; 4],
    /// MainDepth capture (typeless copy, depth-readable SRV).
    depth: Option<Capture>,
    /// Raw (non-sRGB) RTVs over the engine's G-buffer textures, keyed by texture pointer so an
    /// engine resize rebuilds them.
    raw_rtvs: [Option<(usize, ID3D11RenderTargetView)>; 4],
    /// Whether a far frame has been captured since the pipeline (re)build.
    captured: bool,
}

impl SharePipeline {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("far field: creating the vertex shader")?;
            let vertex_shader =
                vertex_shader.context("far field: the vertex shader was not created")?;

            let mut composite_shader = None;
            d3d.CreatePixelShader(COMPOSITE_DXBC, None, Some(&mut composite_shader))
                .context("far field: creating the composite pixel shader")?;
            let composite_shader = composite_shader
                .context("far field: the composite pixel shader was not created")?;

            let mut depth_merge_state = None;
            d3d.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC {
                    DepthEnable: true.into(),
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
                    DepthFunc: D3D11_COMPARISON_GREATER_EQUAL,
                    StencilEnable: false.into(),
                    ..Default::default()
                },
                Some(&mut depth_merge_state),
            )
            .context("far field: creating the depth-merge state")?;
            let depth_merge_state =
                depth_merge_state.context("far field: the depth-merge state was not created")?;

            let mut rasterizer = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("far field: creating the rasterizer state")?;
            let rasterizer =
                rasterizer.context("far field: the rasterizer state was not created")?;

            // Default-constructed blend desc = blending disabled; only the write mask needs
            // setting (independent blend off, so RT[0] applies to all four targets).
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0].RenderTargetWriteMask = 0x0F;
            let mut opaque_blend = None;
            d3d.CreateBlendState(&blend_desc, Some(&mut opaque_blend))
                .context("far field: creating the opaque blend state")?;
            let opaque_blend =
                opaque_blend.context("far field: the opaque blend state was not created")?;

            let params = [0.0f32; 4];
            let mut params_cb = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 16,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: windows::Win32::Graphics::Direct3D11::D3D11_BIND_CONSTANT_BUFFER.0
                        as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: params.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut params_cb),
            )
            .context("far field: creating the params constant buffer")?;
            let params_cb = params_cb.context("far field: the params buffer was not created")?;

            Ok(Self {
                vertex_shader,
                composite_shader,
                depth_merge_state,
                opaque_blend,
                rasterizer,
                params_cb,
                gbuffer: [None, None, None, None],
                depth: None,
                raw_rtvs: [None, None, None, None],
                captured: false,
            })
        }
    }

    /// Copy the live MainDepth + GBuffer0..3 into the capture textures.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn capture(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
    ) -> anyhow::Result<()> {
        let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
            .context("far field: the graphics engine is unavailable")?;
        // GBuffer0..3: same-underlying-format copies (from the live texture descriptor, so
        // typeless/sRGB families copy exactly), sampled through raw non-sRGB views.
        for (i, slot) in self.gbuffer.iter_mut().enumerate() {
            let tex = unsafe { ge.m_GBufferTexture[i].as_ref() }
                .with_context(|| format!("far field: GBuffer{i} is unavailable"))?;
            let src = crate::vr::foveation::resource_as_texture(&raw const tex.m_Texture)
                .with_context(|| format!("far field: GBuffer{i} has no texture resource"))?;
            let mut src_desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { src.GetDesc(&mut src_desc) };
            let desc = (src_desc.Width, src_desc.Height, src_desc.Format.0);
            if slot.as_ref().map(|c| c.desc) != Some(desc) {
                let srv_format = raw_color_format(src_desc.Format).unwrap_or(src_desc.Format);
                *slot = Some(create_capture(device, desc, src_desc.Format, srv_format)?);
            }
            let capture = slot.as_ref().expect("the capture was just ensured");
            unsafe { context.CopyResource(&capture.texture, &src) };
        }
        // MainDepth: a typeless copy in the depth format's family, with a depth-readable SRV. The
        // engine's D32FS8 maps to the R32G8X24 family; a D24S8 build would map to R24G8 (handled
        // for robustness).
        {
            let tex = unsafe { ge.m_MainDepthTexture.as_ref() }
                .context("far field: MainDepth is unavailable")?;
            let desc = (tex.m_Width as u32, tex.m_Height as u32, tex.m_Format as i32);
            if self.depth.as_ref().map(|c| c.desc) != Some(desc) {
                let src = crate::vr::foveation::resource_as_texture(&raw const tex.m_Texture)
                    .context("far field: MainDepth has no texture resource")?;
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                unsafe { src.GetDesc(&mut src_desc) };
                let (copy_format, srv_format) = depth_family_formats(src_desc.Format)
                    .with_context(|| {
                        format!("far field: unsupported depth format {:?}", src_desc.Format)
                    })?;
                self.depth = Some(create_capture(device, desc, copy_format, srv_format)?);
            }
            let capture = self.depth.as_ref().expect("the capture was just ensured");
            let src = crate::vr::foveation::resource_as_texture(&raw const tex.m_Texture)
                .context("far field: MainDepth has no texture resource")?;
            unsafe { context.CopyResource(&capture.texture, &src) };
        }
        self.captured = true;
        Ok(())
    }

    /// Draw the composite full-screen pass into the currently bound targets' textures for `eye`.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context, held under its mutex.
    unsafe fn composite(
        &mut self,
        context: &ID3D11DeviceContext,
        eye: usize,
    ) -> anyhow::Result<()> {
        if !self.captured {
            anyhow::bail!("no far frame captured yet");
        }
        let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
            .context("far field: the graphics engine is unavailable")?;

        // Output targets: raw (non-sRGB) RTVs over the engine's G-buffer textures — pairing with
        // the raw capture SRVs so the composite is a byte pass-through (see `raw_color_format`) —
        // plus the main depth DSV.
        let mut rtvs: [Option<ID3D11RenderTargetView>; 4] = std::array::from_fn(|_| None);
        for (i, rtv) in rtvs.iter_mut().enumerate() {
            let tex = unsafe { ge.m_GBufferTexture[i].as_ref() }
                .with_context(|| format!("far field: GBuffer{i} is unavailable"))?;
            let src = crate::vr::foveation::resource_as_texture(&raw const tex.m_Texture)
                .with_context(|| format!("far field: GBuffer{i} has no texture resource"))?;
            let key = tex as *const _ as usize;
            if self.raw_rtvs[i].as_ref().map(|(k, _)| *k) != Some(key) {
                let mut src_desc = D3D11_TEXTURE2D_DESC::default();
                unsafe { src.GetDesc(&mut src_desc) };
                let format = raw_color_format(src_desc.Format).unwrap_or(src_desc.Format);
                use windows::Win32::Graphics::Direct3D11::{
                    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RENDER_TARGET_VIEW_DESC_0,
                    D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_TEX2D_RTV,
                };
                let mut view = None;
                unsafe {
                    ge.m_Device
                        .as_ref()
                        .context("far field: the graphics device is unavailable")?
                        .m_Device
                        .CreateRenderTargetView(
                            &src,
                            Some(&D3D11_RENDER_TARGET_VIEW_DESC {
                                Format: format,
                                ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                                Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                                    Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
                                },
                            }),
                            Some(&mut view),
                        )
                        .with_context(|| format!("far field: creating GBuffer{i}'s raw RTV"))?;
                }
                let view = view
                    .with_context(|| format!("far field: GBuffer{i}'s raw RTV was not created"))?;
                self.raw_rtvs[i] = Some((key, view));
            }
            *rtv = self.raw_rtvs[i].as_ref().map(|(_, v)| v.clone());
        }
        let depth_tex = unsafe { ge.m_MainDepthTexture.as_ref() }
            .context("far field: MainDepth is unavailable")?;
        let dsv = crate::vr::foveation::view_ptr(&raw const depth_tex.m_DSV)
            .context("far field: MainDepth has no DSV")?;
        let (width, height) = (depth_tex.m_Width as f32, depth_tex.m_Height as f32);

        // The per-eye NDC affine from the eye's off-axis projection into the far image's (eye 0's).
        let (scale, offset) = ndc_affine(eye);
        // SAFETY: dynamic CB map/write of exactly the buffer's 16 bytes.
        unsafe {
            let mut mapped = Default::default();
            context
                .Map(
                    &self.params_cb,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .context("far field: mapping the params buffer")?;
            let data: [f32; 4] = [scale.0, scale.1, offset.0, offset.1];
            std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData.cast(), 4);
            context.Unmap(&self.params_cb, 0);
        }

        let srvs: [Option<ID3D11ShaderResourceView>; 5] = [
            self.gbuffer[0].as_ref().map(|c| c.srv.clone()),
            self.gbuffer[1].as_ref().map(|c| c.srv.clone()),
            self.gbuffer[2].as_ref().map(|c| c.srv.clone()),
            self.gbuffer[3].as_ref().map(|c| c.srv.clone()),
            self.depth.as_ref().map(|c| c.srv.clone()),
        ];
        if srvs.iter().any(Option::is_none) {
            anyhow::bail!("a far capture target is missing");
        }

        // SAFETY: standard full-screen pass against the live context; all prior pipeline state the
        // pass overwrites is captured and restored.
        unsafe {
            let backup = crate::vr::foveation::capture_state(context);
            context.OMSetRenderTargets(Some(&rtvs), Some(&dsv));
            context.OMSetDepthStencilState(Some(&self.depth_merge_state), 0);
            context.OMSetBlendState(Some(&self.opaque_blend), None, 0xFFFF_FFFF);
            context.RSSetState(Some(&self.rasterizer));
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                Width: width,
                Height: height,
                MaxDepth: 1.0,
                ..Default::default()
            }]));
            context.IASetPrimitiveTopology(
                windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            );
            context.IASetInputLayout(None);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.composite_shader, None);
            context.PSSetConstantBuffers(0, Some(&[Some(self.params_cb.clone())]));
            context.PSSetShaderResources(0, Some(&srvs));
            context.Draw(3, 0);
            // Unbind our SRVs before the geometry passes rebind these textures as targets.
            let null_srvs: [Option<ID3D11ShaderResourceView>; 5] = std::array::from_fn(|_| None);
            context.PSSetShaderResources(0, Some(&null_srvs));
            crate::vr::foveation::restore_state(context, &backup);
        }
        Ok(())
    }
}

/// The per-axis NDC affine mapping the output eye's projection into the far image's (eye 0's):
/// with row-vector off-axis projections sharing `w = -z`, `ndc_far = ndc_eye * scale + offset`
/// with `scale = P_far[d] / P_eye[d]` and `offset = P_eye[shear] * scale - P_far[shear]` per axis.
/// Identity when either projection is unavailable (flatscreen stereo: identical projections).
fn ndc_affine(eye: usize) -> ((f32, f32), (f32, f32)) {
    let (Some(p_eye), Some(p_far)) = (
        crate::vr::render_params(eye).map(|p| p.projection_standard),
        crate::vr::render_params(0).map(|p| p.projection_standard),
    ) else {
        return ((1.0, 1.0), (0.0, 0.0));
    };
    let sx = p_far.data[0] / p_eye.data[0];
    let sy = p_far.data[5] / p_eye.data[5];
    let ox = p_eye.data[8] * sx - p_far.data[8];
    let oy = p_eye.data[9] * sy - p_far.data[9];
    ((sx, sy), (ox, oy))
}

/// Create one capture target: a texture of `copy_format` sized per `desc`, with an SRV of
/// `srv_format`.
fn create_capture(
    device: &Device,
    desc: (u32, u32, i32),
    copy_format: DXGI_FORMAT,
    srv_format: DXGI_FORMAT,
) -> anyhow::Result<Capture> {
    // SAFETY: the engine device is live; the descriptors are valid.
    unsafe {
        let mut texture = None;
        device
            .m_Device
            .CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: desc.0,
                    Height: desc.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: copy_format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut texture),
            )
            .context("far field: creating a capture texture")?;
        let texture = texture.context("far field: the capture texture was not created")?;

        let srv = if srv_format == copy_format {
            let mut srv = None;
            device
                .m_Device
                .CreateShaderResourceView(&texture, None, Some(&mut srv))
                .context("far field: creating a capture SRV")?;
            srv
        } else {
            use windows::Win32::Graphics::{
                Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D,
                Direct3D11::{
                    D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0,
                    D3D11_TEX2D_SRV,
                },
            };
            let mut srv = None;
            device
                .m_Device
                .CreateShaderResourceView(
                    &texture,
                    Some(&D3D11_SHADER_RESOURCE_VIEW_DESC {
                        Format: srv_format,
                        ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_SRV {
                                MostDetailedMip: 0,
                                MipLevels: 1,
                            },
                        },
                    }),
                    Some(&mut srv),
                )
                .context("far field: creating the depth capture SRV")?;
            srv
        };
        let srv = srv.context("far field: the capture SRV was not created")?;
        Ok(Capture { texture, srv, desc })
    }
}

/// The non-sRGB typed view format for a colour texture's family: the composite must be a pure
/// byte pass-through (sample raw, write raw), because the engine's own G-buffer views can be sRGB
/// (GBuffer0 carries sRGB and linear aliases) — sampling raw bytes through a linear view and then
/// writing through the engine's sRGB view double-encodes the albedo, darkening every composited
/// pixel. `None` means the format is already a plain typed format usable as-is.
fn raw_color_format(format: DXGI_FORMAT) -> Option<DXGI_FORMAT> {
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        DXGI_FORMAT_R10G10B10A2_TYPELESS, DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R16G16B16A16_TYPELESS, DXGI_FORMAT_R16G16B16A16_UNORM,
    };
    match format {
        DXGI_FORMAT_R8G8B8A8_TYPELESS | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
            Some(DXGI_FORMAT_R8G8B8A8_UNORM)
        }
        DXGI_FORMAT_B8G8R8A8_TYPELESS | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => {
            Some(DXGI_FORMAT_B8G8R8A8_UNORM)
        }
        DXGI_FORMAT_R10G10B10A2_TYPELESS => Some(DXGI_FORMAT_R10G10B10A2_UNORM),
        DXGI_FORMAT_R16G16B16A16_TYPELESS => Some(DXGI_FORMAT_R16G16B16A16_UNORM),
        _ => None,
    }
}

/// The typeless copy format and depth-readable SRV format for a depth texture's family.
fn depth_family_formats(format: DXGI_FORMAT) -> Option<(DXGI_FORMAT, DXGI_FORMAT)> {
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
    };
    match format {
        DXGI_FORMAT_D32_FLOAT_S8X24_UINT | DXGI_FORMAT_R32G8X24_TYPELESS => Some((
            DXGI_FORMAT_R32G8X24_TYPELESS,
            DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS,
        )),
        DXGI_FORMAT_D24_UNORM_S8_UINT | DXGI_FORMAT_R24G8_TYPELESS => Some((
            DXGI_FORMAT_R24G8_TYPELESS,
            DXGI_FORMAT_R24_UNORM_X8_TYPELESS,
        )),
        _ => None,
    }
}
