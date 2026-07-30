//! The stereo swapchain: creation at the runtime's recommended per-eye resolution, format
//! negotiation, and the per-frame image acquire/release the per-eye blit renders into.

use anyhow::Context as _;
use openxr as xr;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};

use crate::vr::{VIEW_COUNT, VIEW_TYPE, VrConfig, eye_resolution::scaled_eye_size};

/// The stereo swapchain: a single 2-slice texture array (one slice per eye), sized from the
/// runtime's recommended per-eye resolution scaled by `vr.resolution_scale`, in a negotiated format.
pub(super) struct Swapchain {
    pub(super) handle: xr::Swapchain<xr::D3D11>,
    pub(super) width: u32,
    pub(super) height: u32,
    /// The DXGI format actually chosen (recorded for the per-eye blit, which must match/convert).
    pub(super) format: u32,
    /// The enumerated swapchain images (raw `ID3D11Texture2D` pointers as `usize`, so the state stays
    /// `Send`; runtime-owned). Cast back to a pointer at [`Swapchain::acquired_texture`].
    images: Vec<usize>,
    /// The index returned by the most recent `acquire_image`, valid until `release_image`.
    acquired_index: Option<u32>,
}

impl Swapchain {
    /// Create the swapchain from the recommended view-configuration resolution × `resolution_scale`,
    /// negotiating a format from `enumerate_swapchain_formats` (preferring sRGB 8-bit).
    pub(super) fn create(
        instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::D3D11>,
        cfg: &VrConfig,
    ) -> anyhow::Result<Self> {
        let views = instance
            .enumerate_view_configuration_views(system, VIEW_TYPE)
            .context("vr: enumerating view configuration views")?;
        let view = views
            .first()
            .context("vr: the runtime reported no view configuration views")?;

        let (width, height) = scaled_eye_size(
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
            cfg.resolution_scale,
        );

        let formats = session
            .enumerate_swapchain_formats()
            .context("vr: enumerating swapchain formats")?;
        let format = negotiate_format(&formats)?;

        let handle = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::SAMPLED
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format,
                sample_count: 1,
                width,
                height,
                face_count: 1,
                array_size: VIEW_COUNT,
                mip_count: 1,
            })
            .context("vr: create_swapchain failed")?;

        let images: Vec<usize> = handle
            .enumerate_images()
            .context("vr: enumerating swapchain images")?
            .into_iter()
            .map(|ptr| ptr as usize)
            .collect();

        tracing::info!(
            target: "vr",
            width,
            height,
            format,
            image_count = images.len(),
            "created the stereo swapchain",
        );

        Ok(Self {
            handle,
            width,
            height,
            format,
            images,
            acquired_index: None,
        })
    }

    pub(super) fn acquire(&mut self) -> anyhow::Result<()> {
        let index = self
            .handle
            .acquire_image()
            .context("vr: acquire_image failed")?;
        self.handle
            .wait_image(xr::Duration::INFINITE)
            .context("vr: wait_image failed")?;
        self.acquired_index = Some(index);
        Ok(())
    }

    pub(super) fn release(&mut self) -> anyhow::Result<()> {
        self.handle
            .release_image()
            .context("vr: release_image failed")?;
        self.acquired_index = None;
        Ok(())
    }

    /// The currently acquired texture, or `None` when no image is acquired.
    pub(super) fn acquired_texture(&self) -> Option<*mut std::ffi::c_void> {
        let index = self.acquired_index? as usize;
        self.images.get(index).map(|&p| p as *mut std::ffi::c_void)
    }
}

/// Negotiate a swapchain color format from the runtime's supported list, preferring an 8-bit sRGB
/// format (the eye captures resolve through the engine's LDR path). Falls back to the runtime's
/// first offered format, logging the choice. The game's captures may be a different format; the
/// per-eye blit bridges them.
fn negotiate_format(formats: &[u32]) -> anyhow::Result<u32> {
    const PREFERRED: [u32; 2] = [
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32,
    ];

    if let Some(&format) = PREFERRED.iter().find(|f| formats.contains(f)) {
        tracing::info!(target: "vr", format, "negotiated a preferred sRGB swapchain format");
        return Ok(format);
    }
    let format = *formats
        .first()
        .context("vr: the runtime offered no swapchain formats")?;
    tracing::warn!(
        target: "vr",
        format,
        "no preferred sRGB format available; using the runtime's first offered format",
    );
    Ok(format)
}
