//! A 1x1 readback of the scene depth under the reticle, for the center layer's aim depth.
//!
//! The grapple reticle supplies an aim depth only while grappling; the depth buffer knows what
//! the crosshair is over at all times. Each frame (eye 0) the probe copies the center texel of
//! [`GraphicsEngine::m_MainDepthTexture`] into one of two 1x1 staging textures and maps the one
//! written the previous frame, so the readback never stalls the GPU. The raw reverse-Z value is
//! converted to a view-space distance by the caller (which has the projection).

use anyhow::Context as _;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11DeviceContext, ID3D11Texture2D,
    },
    Dxgi::Common::DXGI_FORMAT,
};

/// The double-buffered staging pair and its bookkeeping.
pub(super) struct DepthProbe {
    staging: [ID3D11Texture2D; 2],
    /// The staging texture the next copy writes into; the other one is mapped.
    parity: usize,
    /// Whether each staging texture holds a pending copy from an earlier frame.
    primed: [bool; 2],
    /// The source format the staging pair was built for; a change rebuilds the pair.
    format: DXGI_FORMAT,
}

impl DepthProbe {
    /// Build a staging pair matching `source`'s format. Fails on multisampled sources (the
    /// engine's main depth is single-sample; MSAA would need a resolve first).
    pub fn new(
        device: &jc3gi::graphics_engine::device::Device,
        source: &ID3D11Texture2D,
    ) -> anyhow::Result<Self> {
        // SAFETY: `source` is the live engine depth texture; the descriptor is filled by GetDesc.
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            if desc.SampleDesc.Count > 1 {
                anyhow::bail!("depth probe: the main depth texture is multisampled");
            }
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: 1,
                Height: 1,
                MipLevels: 1,
                ArraySize: 1,
                Format: desc.Format,
                SampleDesc: desc.SampleDesc,
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let make = || -> anyhow::Result<ID3D11Texture2D> {
                let mut texture = None;
                device
                    .m_Device
                    .CreateTexture2D(&staging_desc, None, Some(&mut texture))
                    .context("creating a depth-probe staging texture")?;
                texture.context("the depth-probe staging texture was not created")
            };
            Ok(Self {
                staging: [make()?, make()?],
                parity: 0,
                primed: [false, false],
                format: desc.Format,
            })
        }
    }

    /// Whether the pair matches `format` (rebuild when the engine's depth format changes).
    pub fn matches(&self, source: &ID3D11Texture2D) -> bool {
        // SAFETY: GetDesc on a live texture.
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            source.GetDesc(&mut desc);
            desc.Format == self.format
        }
    }

    /// Queue a copy of `source`'s center texel and return the raw depth value copied on an
    /// earlier frame, or `None` while the pipeline is priming. The caller must hold the engine
    /// context mutex.
    pub fn sample(
        &mut self,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        width: u32,
        height: u32,
    ) -> Option<f32> {
        let write = self.parity;
        let read = 1 - self.parity;
        // SAFETY: both textures are live; the box is a 1x1 region inside the source; the mapped
        // read is of a staging texture whose copy was issued at least a frame ago.
        unsafe {
            let x = width / 2;
            let y = height / 2;
            context.CopySubresourceRegion(
                &self.staging[write],
                0,
                0,
                0,
                0,
                source,
                0,
                Some(&D3D11_BOX {
                    left: x,
                    top: y,
                    front: 0,
                    right: x + 1,
                    bottom: y + 1,
                    back: 1,
                }),
            );
            self.primed[write] = true;
            self.parity = read;

            if !self.primed[read] {
                return None;
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            if context
                .Map(&self.staging[read], 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .is_err()
            {
                return None;
            }
            // Both D32-family formats carry the float depth in the first four bytes of the texel.
            let depth = (mapped.pData as *const f32).read_unaligned();
            context.Unmap(&self.staging[read], 0);
            Some(depth)
        }
    }
}
