//! Draw the redirected HUD texture as a lazy-follow in-scene quad, per eye, over the final image.
//!
//! Step one redirected the HUD into our texture and dropped it from the scene composite; this draws it
//! back in as a head-relative panel that lazily follows the head's orientation with critically-damped
//! quaternion slerp. The panel sits at its own world-space orientation (the damped follow rotation),
//! positioned at the camera position + panel_forward * distance. Corners are uploaded alongside the
//! camera's per-eye view-projection matrix; the vertex shader projects each corner
//! ([`hud_quad_vs`](../shaders/hud_quad_vs.hlsl) / `hud_quad_ps`). The panel is an alpha-blended overlay
//! with the depth test disabled, drawn onto the linear back buffer at the end of the eye's draw.
//!
//! World-space corners are computed once per frame (eye 0) via [`compute_world_corners`] and cached
//! by [`super::state::HudState`]; both eyes then project the same world-space quad through their own
//! per-eye VP. Geometry comes from [`crate::hud::config::HudConfig`]; the panel pose (head-following
//! or world-static, per the [`crate::hud::HudMode`]) is chosen by
//! [`super::state::HudState::update_pose`].

use anyhow::Context as _;
use glam::{Quat, Vec3};
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

/// Constant buffer uploaded per draw: view-projection matrix followed by four world-space corners.
/// Matches the HLSL `cbuffer Quad` layout: `row_major float4x4 ViewProjection` (64 bytes) +
/// `float4 Corners[4]` (64 bytes).
#[repr(C)]
struct QuadConstants {
    view_projection: [f32; 16],
    corners: [[f32; 4]; 4], // .xyz = world-space position, .w = unused
}

/// The quad pass: the textured-quad pipeline and a constant buffer for the per-eye draw data.
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
                    ByteWidth: size_of::<QuadConstants>() as u32,
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

    /// Draw the HUD panel over `target` (the eye's linear back buffer), sampling `hud_srv` (the
    /// redirected HUD texture). `corners` are the pre-computed world-space quad corners (computed
    /// once per frame by [`compute_world_corners`]). The per-eye view-projection is read from the
    /// render camera at draw time. The caller must hold the engine context mutex. Returns `false`
    /// on failure.
    pub fn draw(
        &self,
        context: &ID3D11DeviceContext,
        device: &Device,
        target: &Texture,
        hud_srv: &ID3D11ShaderResourceView,
        corners: &[[f32; 4]; 4],
    ) -> bool {
        let width = u32::from(target.m_Width);
        let height = u32::from(target.m_Height);
        if width == 0 || height == 0 {
            return false;
        }

        let Some(view_proj) = fetch_view_projection() else {
            return false;
        };

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

            let constants = QuadConstants {
                view_projection: view_proj,
                corners: *corners,
            };

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
            std::ptr::copy_nonoverlapping(
                &constants as *const QuadConstants as *const u8,
                mapped.pData as *mut u8,
                size_of::<QuadConstants>(),
            );
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

/// Geometry parameters for computing the panel's world-space corners. Bundled into a struct to keep
/// the argument list under `clippy::too_many_arguments`.
pub(crate) struct PanelParams {
    /// The panel's anchor position (the eye/head position the panel is placed in front of). In
    /// [`HudMode::Hud`](crate::hud::HudMode::Hud) this tracks the head; in
    /// [`HudMode::Movie`](crate::hud::HudMode::Movie) it is the latched world-static position.
    pub pos: Vec3,
    /// The panel's world orientation: the damped follow rotation in `Hud` mode, or the latched
    /// rotation in `Movie` mode.
    pub rot: Quat,
    /// Panel aspect ratio (width / height) -- the effective aspect for the current mode, shared with
    /// the render target and marker projection so the panel always matches the texture.
    pub aspect: f32,
    pub distance: f32,
    pub panel_height: f32,
}

/// Compute the panel's world-space corners from the supplied pose. Call once per frame (eye 0) and
/// cache the result so both eyes project the same world-space quad through their own per-eye VP.
/// Returns `None` only for a degenerate aspect.
///
/// The panel sits at its own world-space orientation (`rot`), positioned at `pos + forward *
/// distance`. It is NOT rotated through the camera's transform — that would stack the rotation on
/// top of the head rotation, swinging the panel offscreen on large turns.
pub(crate) fn compute_world_corners(params: &PanelParams) -> Option<[[f32; 4]; 4]> {
    if params.aspect <= 0.0 {
        return None;
    }
    let aspect = params.aspect;

    // Panel basis vectors from the pose quaternion. The quaternion represents the same rotation as a
    // camera world transform, so forward = quat * -Z matches the head's forward direction.
    let forward = params.rot * Vec3::NEG_Z;
    let right = params.rot * Vec3::X;
    let up = params.rot * Vec3::Y;

    let center = params.pos + forward * params.distance;
    let half_h = params.panel_height * 0.5;
    let half_w = params.panel_height * aspect * 0.5;

    let layout = [
        (-half_w, half_h),
        (half_w, half_h),
        (-half_w, -half_h),
        (half_w, -half_h),
    ];

    Some(layout.map(|(dx, dy)| {
        let corner = center + right * dx + up * dy;
        [corner.x, corner.y, corner.z, 1.0]
    }))
}

/// Fetch the render camera's view-projection matrix for the current eye.
fn fetch_view_projection() -> Option<[f32; 16]> {
    unsafe {
        let cm = jc3gi::camera::camera_manager::CameraManager::get()?;
        let cam = cm.m_RenderCamera.as_ref()?;
        Some(cam.m_ViewProjectionF.data)
    }
}
