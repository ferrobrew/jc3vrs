//! The side-by-side composite pass: a fullscreen-triangle pipeline that samples each eye's
//! back-buffer capture into its half of the capture swapchain's back buffer.
//!
//! The pipeline is a single VS (generated from `SV_VertexID`, no vertex buffer or input layout)
//! and a trivial sample PS, plus a no-cull rasterizer and a clamped linear sampler. Two draw calls
//! per frame -- one per eye -- each scoped to a half-width viewport. The eye-to-half mapping is
//! swapped when `cross_eyed` is set, so the pair fuses cross-eyed instead of parallel. The pipeline
//! state is created once and reused; only the bound SRV and viewport change per draw.

use anyhow::Context as _;
use jc3gi::graphics_engine::device::Device;
use windows::Win32::Graphics::{
    Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    Direct3D11::{
        D3D11_COMPARISON_NEVER, D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        D3D11_RASTERIZER_DESC, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_VIEWPORT,
        ID3D11DeviceContext, ID3D11PixelShader, ID3D11RasterizerState, ID3D11RenderTargetView,
        ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11VertexShader,
    },
};

/// The committed, precompiled composite shaders (entry point `main`).
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/capture_ps.dxbc");

/// The composite pass: shader pipeline + sampler + rasterizer. Built once on first use.
pub(super) struct CaptureComposite {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
}

impl CaptureComposite {
    /// Build the composite pipeline against the engine's D3D11 device.
    pub(super) fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors below are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("creating the capture vertex shader")?;
            let vertex_shader =
                vertex_shader.context("the capture vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("creating the capture pixel shader")?;
            let pixel_shader = pixel_shader.context("the capture pixel shader was not created")?;

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
            .context("creating the capture sampler")?;
            let sampler = sampler.context("the capture sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("creating the capture rasterizer state")?;
            let rasterizer = rasterizer.context("the capture rasterizer state was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
            })
        }
    }

    /// Composite both eyes into `rtv` (the capture swapchain's back buffer), one eye per half. The
    /// caller must hold the engine context mutex. `back_size` is the back-buffer dimensions;
    /// `eye0_srv`/`eye1_srv` are the per-eye capture SRVs (either may be `None` if that eye was not
    /// captured this frame, in which case its half is left cleared to the clear colour).
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `rtv` must be a valid RTV over the
    /// capture swapchain's back buffer.
    pub(super) unsafe fn draw(
        &self,
        context: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        back_size: (u32, u32),
        eye0_srv: Option<&ID3D11ShaderResourceView>,
        eye1_srv: Option<&ID3D11ShaderResourceView>,
        cross_eyed: bool,
    ) {
        let (back_w, back_h) = back_size;
        if back_w == 0 || back_h == 0 {
            return;
        }
        let half_w = back_w / 2;

        // Cross-eyed swaps the eye-to-half mapping: right eye on the left, left eye on the right.
        let (left_eye, right_eye) = if cross_eyed {
            (eye1_srv, eye0_srv)
        } else {
            (eye0_srv, eye1_srv)
        };

        unsafe {
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);

            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.RSSetState(&self.rasterizer);

            // Left half.
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: half_w as f32,
                Height: back_h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            if let Some(srv) = left_eye {
                context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
                context.Draw(3, 0);
            }

            // Right half.
            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: half_w as f32,
                TopLeftY: 0.0,
                Width: half_w as f32,
                Height: back_h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            if let Some(srv) = right_eye {
                context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
                context.Draw(3, 0);
            }

            // Unbind so the engine's next pass does not see our SRV/RTV still bound.
            context.PSSetShaderResources(0, Some(&[None]));
            context.OMSetRenderTargets(Some(&[None]), None);
        }
    }
}
