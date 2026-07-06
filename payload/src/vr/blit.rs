//! The per-eye blit: copy each captured eye texture into its slice of the OpenXR stereo swapchain,
//! then submit the frame.
//!
//! The game renders each eye into `m_BackBufferLinear` and the mod captures it after the resolve
//! (`docs/engine/rendering.md` §12); those captures live in [`EGUI_DEBUG_RENDER_STATE`] as
//! `R8G8B8A8_UNORM` textures sized to the game's back buffer. The OpenXR swapchain is a 2-slice
//! texture array sized to the runtime's recommended per-eye resolution in a negotiated (usually
//! `_SRGB`) format. Sizes and formats generally differ, so this is a **shader blit** (fullscreen
//! triangle sampling the eye SRV into the slice RTV), not a `CopyResource`. It runs on the engine's
//! immediate context under `Context::m_Mutex`, serialized with the engine's own render work, the
//! same model as [`crate::capture::composite`].
//!
//! The gamma bridge (linear source vs `_SRGB` target) is a runtime toggle; see
//! [`crate::vr::config::BlitGamma`] and `vr_blit_ps.hlsl`.

use anyhow::Context as _;
use parking_lot::Mutex;
use windows::{
    Win32::{
        Graphics::{
            Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER,
                D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_VIEW_DESC,
                D3D11_RENDER_TARGET_VIEW_DESC_0, D3D11_RTV_DIMENSION_TEXTURE2DARRAY,
                D3D11_SAMPLER_DESC, D3D11_SUBRESOURCE_DATA, D3D11_TEX2D_ARRAY_RTV,
                D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                D3D11_VIEWPORT, ID3D11Buffer, ID3D11DeviceContext, ID3D11PixelShader,
                ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11SamplerState,
                ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::Common::DXGI_FORMAT,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

use jc3gi::graphics_engine::device::Device;

use crate::ui::render::EGUI_DEBUG_RENDER_STATE;

use super::{FrameContext, VrConfig, config::BlitGamma};

/// The committed, precompiled blit shaders. The vertex shader is the shared fullscreen-triangle from
/// the capture composite (entry point `main`); the pixel shader is VR-specific (gamma bridge).
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/vr_blit_ps.dxbc");

/// The blit pipeline singleton, built lazily on the first submitted VR frame and torn down with the
/// runtime. Holds COM objects, which `windows` marks `Send`/`Sync`, so a `Mutex` static is sound.
static VR_BLIT: Mutex<Option<VrBlit>> = Mutex::new(None);

/// Acquire the swapchain image, blit both captured eyes into their slices, release, and submit the
/// frame. Consumes the [`FrameContext`], releasing the OpenXR runtime lock. The frame is always
/// ended -- on any blit failure it submits an empty frame rather than leaving the compositor waiting.
pub fn present_and_submit(mut frame: FrameContext, cfg: &VrConfig) {
    if !frame.should_render() {
        end(frame);
        return;
    }

    if let Err(e) = submit(&mut frame, cfg) {
        tracing::warn!(target: "vr", "eye blit failed; submitting an empty frame: {e:#}");
        // Release any acquired image so the empty submit does not deadlock the swapchain.
        let _ = frame.release();
    }
    end(frame);
}

/// Tear down the blit pipeline (COM release). Called from the runtime cleanup.
pub fn teardown() {
    *VR_BLIT.lock() = None;
}

/// End the frame, logging a submit failure (the frame is consumed regardless).
fn end(frame: FrameContext) {
    if let Err(e) = frame.frame_end() {
        tracing::warn!(target: "vr", "frame end failed: {e:#}");
    }
}

/// The acquire → blit → release body, factored out so [`present_and_submit`] can end the frame on
/// either success or failure.
fn submit(frame: &mut FrameContext, cfg: &VrConfig) -> anyhow::Result<()> {
    // Clone (AddRef) the captured eye textures under a brief EGUI lock, dropped before the blit
    // state / context work, mirroring `capture::present_active`'s lock discipline.
    let eye_textures: [Option<ID3D11Texture2D>; 2] = {
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        [lock.texture(0).cloned(), lock.texture(1).cloned()]
    };

    frame
        .acquire()
        .context("vr: acquiring the swapchain image")?;

    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable for the blit")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("vr: the graphics device is unavailable")?;
    let context =
        unsafe { device.m_Context.as_ref() }.context("vr: the graphics context is unavailable")?;

    let linearize = matches!(cfg.blit_srgb_gamma, BlitGamma::Linearize);

    let mut guard = VR_BLIT.lock();
    if guard.is_none() {
        *guard = Some(VrBlit::new(device)?);
    }
    let blit = guard.as_mut().expect("the blit pipeline was just ensured");

    // Both eyes' immediate-context work runs under one critical-section hold, serialized with the
    // engine's render work.
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        blit.set_gamma(&context.m_Context, linearize);
        for (eye, src) in eye_textures.iter().enumerate() {
            let (Some(image), Some(src)) = (frame.eye_image(eye), src.as_ref()) else {
                continue;
            };
            if let Err(e) = blit.blit_eye(device, &context.m_Context, &image, src) {
                tracing::warn!(target: "vr", eye, "eye blit draw failed: {e:#}");
            }
        }
        LeaveCriticalSection(context.m_Mutex);
    }
    drop(guard);

    frame
        .release()
        .context("vr: releasing the swapchain image")?;
    Ok(())
}

/// The blit pipeline: shaders, sampler, rasterizer, gamma constant buffer, and caches of the
/// per-slice render-target views and per-eye source shader-resource views.
struct VrBlit {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    /// The gamma-mode constant buffer (`b0`), updated per submit.
    gamma_cb: ID3D11Buffer,
    /// Render-target views over swapchain slices, keyed by `(texture pointer, array slice)`. The
    /// swapchain images are enumerated once and stable, so this cache is valid for the runtime's
    /// lifetime.
    rtv_cache: Vec<(usize, u32, ID3D11RenderTargetView)>,
    /// Source shader-resource views over the captured eye textures, keyed by texture pointer, so a
    /// resize (which rebuilds the capture textures) rebuilds the SRV.
    srv_cache: [Option<(usize, ID3D11ShaderResourceView)>; 2],
}

impl VrBlit {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("vr: creating the blit vertex shader")?;
            let vertex_shader =
                vertex_shader.context("vr: the blit vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("vr: creating the blit pixel shader")?;
            let pixel_shader = pixel_shader.context("vr: the blit pixel shader was not created")?;

            let mut sampler: Option<ID3D11SamplerState> = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MinLOD: f32::MIN,
                    MaxLOD: f32::MAX,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 0,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                },
                Some(&mut sampler),
            )
            .context("vr: creating the blit sampler")?;
            let sampler = sampler.context("vr: the blit sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("vr: creating the blit rasterizer state")?;
            let rasterizer = rasterizer.context("vr: the blit rasterizer state was not created")?;

            // 16-byte constant buffer holding the gamma mode in the first uint; the rest is padding.
            let initial: [u32; 4] = [1, 0, 0, 0];
            let mut gamma_cb: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<[u32; 4]>() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: initial.as_ptr().cast(),
                    SysMemPitch: 0,
                    SysMemSlicePitch: 0,
                }),
                Some(&mut gamma_cb),
            )
            .context("vr: creating the blit gamma constant buffer")?;
            let gamma_cb =
                gamma_cb.context("vr: the blit gamma constant buffer was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                gamma_cb,
                rtv_cache: Vec::new(),
                srv_cache: [None, None],
            })
        }
    }

    /// Update the gamma constant buffer. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context.
    unsafe fn set_gamma(&self, context: &ID3D11DeviceContext, linearize: bool) {
        let data: [u32; 4] = [u32::from(linearize), 0, 0, 0];
        unsafe {
            context.UpdateSubresource(&self.gamma_cb, 0, None, data.as_ptr().cast(), 0, 0);
        }
    }

    /// Blit one captured eye into its swapchain slice. The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `image` must reference the currently
    /// acquired swapchain texture and `src` a live capture texture.
    unsafe fn blit_eye(
        &mut self,
        device: &Device,
        context: &ID3D11DeviceContext,
        image: &super::EyeImage,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<()> {
        // Wrap the runtime-owned swapchain texture borrowed (no AddRef); do not release it.
        let swapchain_tex = unsafe { ID3D11Texture2D::from_raw_borrowed(&image.texture) }
            .context("vr: the swapchain texture pointer was null")?;

        let (width, height) = unsafe { texture_size(swapchain_tex) };
        if width == 0 || height == 0 {
            return Ok(());
        }

        let rtv = self.rtv_for(device, swapchain_tex, image)?;
        let srv = self.srv_for(device, image.array_index as usize, src)?;

        unsafe {
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.gamma_cb.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(srv)]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);

            // Unbind so the engine's next pass does not see our SRV/RTV still bound.
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(&[None]), None);
        }
        Ok(())
    }

    /// Get (creating and caching on first use) the render-target view over `image`'s swapchain slice.
    fn rtv_for(
        &mut self,
        device: &Device,
        swapchain_tex: &ID3D11Texture2D,
        image: &super::EyeImage,
    ) -> anyhow::Result<ID3D11RenderTargetView> {
        let key = (image.texture as usize, image.array_index);
        if let Some((_, _, rtv)) = self
            .rtv_cache
            .iter()
            .find(|(p, s, _)| *p == key.0 && *s == key.1)
        {
            return Ok(rtv.clone());
        }
        let desc = D3D11_RENDER_TARGET_VIEW_DESC {
            Format: DXGI_FORMAT(image.format as i32),
            ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2DARRAY,
            Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
                Texture2DArray: D3D11_TEX2D_ARRAY_RTV {
                    MipSlice: 0,
                    FirstArraySlice: image.array_index,
                    ArraySize: 1,
                },
            },
        };
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe {
            device
                .m_Device
                .CreateRenderTargetView(swapchain_tex, Some(&desc), Some(&mut rtv))
        }
        .context("vr: creating the swapchain slice render-target view")?;
        let rtv = rtv.context("vr: the swapchain slice render-target view was not created")?;
        self.rtv_cache.push((key.0, key.1, rtv.clone()));
        Ok(rtv)
    }

    /// Get (creating and caching on first use, rebuilding on a texture change) the source
    /// shader-resource view over the captured eye texture for `eye`.
    fn srv_for(
        &mut self,
        device: &Device,
        eye: usize,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        let ptr = src.as_raw() as usize;
        let slot = &mut self.srv_cache[eye.min(1)];
        if let Some((cached, srv)) = slot.as_ref()
            && *cached == ptr
        {
            return Ok(srv.clone());
        }
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            device
                .m_Device
                .CreateShaderResourceView(src, None, Some(&mut srv))
        }
        .context("vr: creating the captured eye shader-resource view")?;
        let srv = srv.context("vr: the captured eye shader-resource view was not created")?;
        *slot = Some((ptr, srv.clone()));
        Ok(srv)
    }
}

/// The pixel dimensions of a texture from its descriptor.
///
/// # Safety
/// `texture` must be a live `ID3D11Texture2D`.
unsafe fn texture_size(texture: &ID3D11Texture2D) -> (u32, u32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    (desc.Width, desc.Height)
}
