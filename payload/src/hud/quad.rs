//! Draw the redirected HUD texture as a fixed in-scene quad, per eye, over the final image.
//!
//! Step one redirected the HUD into our texture and dropped it from the scene composite; this draws it
//! back in as a head-locked panel floating a fixed distance ahead, with a per-eye horizontal offset so
//! the two eyes get the stereo disparity that places it at that distance. The corners are computed
//! CPU-side ([`quad_corners`]) and uploaded to a constant buffer; the shaders are a trivial textured
//! quad ([`hud_quad_vs`](../shaders/hud_quad_vs.hlsl) / `hud_quad_ps`). The panel is an alpha-blended
//! overlay with the depth test disabled, drawn onto the linear back buffer at the end of the eye's
//! draw.
//!
//! The geometry is hardcoded for now (step two: get it visible); the distance, size, and follow
//! behavior become tunable in step three.

use anyhow::Context as _;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use windows::Win32::Graphics::{
    Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
    Direct3D11::{
        D3D11_BIND_CONSTANT_BUFFER, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
        D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA, D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL,
        D3D11_CPU_ACCESS_WRITE, D3D11_CULL_NONE, D3D11_DEPTH_STENCIL_DESC, D3D11_FILL_SOLID,
        D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
        D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC,
        D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, ID3D11BlendState,
        ID3D11Buffer, ID3D11DepthStencilState, ID3D11DeviceContext, ID3D11PixelShader,
        ID3D11RasterizerState, ID3D11RenderTargetView, ID3D11SamplerState,
        ID3D11ShaderResourceView, ID3D11VertexShader,
    },
};

/// The committed, precompiled quad shaders (entry point `main`).
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/hud_quad_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/hud_quad_ps.dxbc");

/// Distance from the eye to the panel, in meters.
const DISTANCE: f32 = 2.0;
/// Panel height in meters; its width keeps the back-buffer aspect so the HUD is not distorted.
const PANEL_HEIGHT: f32 = 1.4;
/// The panel's stereo separation, in meters (its apparent depth comes from this against `DISTANCE`).
const PANEL_IPD: f32 = 0.064;
/// Vertical field of view used to project the panel, in radians (roughly the game's).
const FOV_Y: f32 = 0.9;

/// The quad pass: the textured-quad pipeline and a constant buffer for the per-eye corners.
pub struct HudQuad {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
    depth_stencil: ID3D11DepthStencilState,
    constants: ID3D11Buffer,
}

impl HudQuad {
    /// Build the quad pipeline and its constant buffer.
    pub fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("creating the HUD quad vertex shader")?;
            let vertex_shader =
                vertex_shader.context("the HUD quad vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("creating the HUD quad pixel shader")?;
            let pixel_shader = pixel_shader.context("the HUD quad pixel shader was not created")?;

            let mut sampler: Option<ID3D11SamplerState> = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )
            .context("creating the HUD quad sampler")?;
            let sampler = sampler.context("the HUD quad sampler was not created")?;

            // Straight (non-premultiplied) alpha so the HUD's transparent regions show the scene.
            let mut blend: Option<ID3D11BlendState> = None;
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            d3d.CreateBlendState(&blend_desc, Some(&mut blend))
                .context("creating the HUD quad blend state")?;
            let blend = blend.context("the HUD quad blend state was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("creating the HUD quad rasterizer state")?;
            let rasterizer = rasterizer.context("the HUD quad rasterizer state was not created")?;

            // Overlay: no depth test, no depth write.
            let mut depth_stencil: Option<ID3D11DepthStencilState> = None;
            d3d.CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC::default(),
                Some(&mut depth_stencil),
            )
            .context("creating the HUD quad depth-stencil state")?;
            let depth_stencil =
                depth_stencil.context("the HUD quad depth-stencil state was not created")?;

            let mut constants: Option<ID3D11Buffer> = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<[[f32; 4]; 4]>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                None,
                Some(&mut constants),
            )
            .context("creating the HUD quad constant buffer")?;
            let constants = constants.context("the HUD quad constant buffer was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                blend,
                rasterizer,
                depth_stencil,
                constants,
            })
        }
    }

    /// Draw the HUD panel for `eye` over `target` (the eye's linear back buffer), sampling `hud_srv`
    /// (the redirected HUD texture). The caller must hold the engine context mutex. Returns `false` on
    /// failure.
    pub fn draw(
        &self,
        context: &ID3D11DeviceContext,
        device: &Device,
        target: &Texture,
        hud_srv: &ID3D11ShaderResourceView,
        eye: usize,
    ) -> bool {
        let width = u32::from(target.m_Width);
        let height = u32::from(target.m_Height);
        if width == 0 || height == 0 {
            return false;
        }
        let aspect = width as f32 / height as f32;

        // SAFETY: `device.m_Device` is live; `target.m_Texture` is the engine's render-target-capable
        // back buffer; we record onto the caller-locked immediate context.
        unsafe {
            let mut rtv: Option<ID3D11RenderTargetView> = None;
            if device
                .m_Device
                .CreateRenderTargetView(&target.m_Texture, None, Some(&mut rtv))
                .is_err()
            {
                return false;
            }
            let Some(rtv) = rtv else {
                return false;
            };

            let corners = quad_corners(eye, aspect);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(
                    &self.constants,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )
                .is_err()
            {
                return false;
            }
            std::ptr::copy_nonoverlapping(corners.as_ptr(), mapped.pData as *mut [f32; 4], 4);
            context.Unmap(&self.constants, 0);

            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.RSSetState(&self.rasterizer);
            context.OMSetRenderTargets(Some(&[Some(rtv)]), None);
            context.OMSetBlendState(&self.blend, None, 0xffff_ffff);
            context.OMSetDepthStencilState(&self.depth_stencil, 0);
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            context.VSSetShader(&self.vertex_shader, None);
            context
                .VSSetConstantBuffers(0, Some(std::slice::from_ref(&Some(self.constants.clone()))));
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetShaderResources(0, Some(std::slice::from_ref(&Some(hud_srv.clone()))));
            context.PSSetSamplers(0, Some(std::slice::from_ref(&Some(self.sampler.clone()))));
            context.Draw(4, 0);

            // Unbind our SRV and RTV so the engine's own passes don't see them still bound.
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(&[None]), None);
            true
        }
    }
}

/// The four clip-space corners (`.xy` = NDC, `.zw` = UV) of the panel for `eye`, given the back-buffer
/// `aspect`. The panel is head-locked at [`DISTANCE`] ahead; each eye is offset by half [`PANEL_IPD`]
/// so the pair converges at that distance. Order is a triangle strip: top-left, top-right, bottom-
/// left, bottom-right.
fn quad_corners(eye: usize, aspect: f32) -> [[f32; 4]; 4] {
    let half_h = PANEL_HEIGHT * 0.5;
    let half_w = PANEL_HEIGHT * aspect * 0.5;
    // Eye 0 = left, eye 1 = right. The panel sits at head origin, so relative to this eye its center
    // shifts opposite the eye offset -- left eye sees it right of center, which converges in front.
    let eye_offset = if eye == 0 {
        -PANEL_IPD * 0.5
    } else {
        PANEL_IPD * 0.5
    };
    let center_x = -eye_offset;

    let tan_y = (FOV_Y * 0.5).tan();
    let tan_x = tan_y * aspect;

    let layout = [
        (-half_w, half_h, 0.0, 0.0),
        (half_w, half_h, 1.0, 0.0),
        (-half_w, -half_h, 0.0, 1.0),
        (half_w, -half_h, 1.0, 1.0),
    ];
    layout.map(|(dx, dy, u, v)| {
        let ndc_x = (center_x + dx) / DISTANCE / tan_x;
        let ndc_y = dy / DISTANCE / tan_y;
        [ndc_x, ndc_y, u, v]
    })
}
