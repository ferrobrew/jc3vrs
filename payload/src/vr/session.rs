//! The OpenXR session and its per-session resources: creation against the game's `ID3D11Device` and
//! the LOCAL reference space. The state machine that drives the session (READY..STOPPING, teardown)
//! lives in [`crate::vr::state`]; persistence across inject cycles in [`crate::vr::persist`].

use anyhow::Context as _;
use openxr as xr;
use windows::core::Interface as _;

use crate::vr::{VrConfig, swapchain::Swapchain};

/// A created OpenXR session and its per-session resources. `running` tracks the READY..STOPPING
/// window driven by the event pump.
pub(super) struct Session {
    pub(super) handle: xr::Session<xr::D3D11>,
    pub(super) frame_wait: xr::FrameWaiter,
    pub(super) frame_stream: xr::FrameStream<xr::D3D11>,
    /// The LOCAL reference space -- the cockpit-relative world frame.
    pub(super) local: xr::Space,
    /// The stereo swapchain, created lazily once the session is running and first rendered.
    pub(super) swapchain: Option<Swapchain>,
    pub(super) running: bool,
}

impl Session {
    /// Create the session against the game's `ID3D11Device`, after checking the D3D11 graphics
    /// requirements the spec requires. The device is fetched from the graphics engine singleton
    /// under [`crate::capture`]'s null-guarding and is not stored.
    pub(super) fn create(
        instance: &xr::Instance,
        system: xr::SystemId,
        _cfg: &VrConfig,
    ) -> anyhow::Result<Self> {
        // The spec requires querying graphics requirements before create_session; the returned
        // min feature level is informational for us (we share the engine's already-created device).
        let requirements = instance
            .graphics_requirements::<xr::D3D11>(system)
            .context("vr: querying D3D11 graphics requirements")?;
        tracing::info!(
            target: "vr",
            min_feature_level = requirements.min_feature_level,
            "D3D11 graphics requirements",
        );

        let device_ptr = with_engine_device(|device| device.m_Device.as_raw())?;

        let (handle, frame_wait, frame_stream) = unsafe {
            instance
                .create_session::<xr::D3D11>(
                    system,
                    &xr::d3d::SessionCreateInfoD3D11 {
                        device: device_ptr.cast(),
                    },
                )
                .context("vr: create_session failed")?
        };

        let local = handle
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("vr: creating the LOCAL reference space")?;

        Ok(Self {
            handle,
            frame_wait,
            frame_stream,
            local,
            swapchain: None,
            running: false,
        })
    }
}

/// Fetch the game's `ID3D11Device` from the graphics engine singleton, null-guarded exactly as
/// [`crate::capture`] does, and run `f` against it. The device is not retained past `f`.
fn with_engine_device<R>(
    f: impl FnOnce(&jc3gi::graphics_engine::device::Device) -> R,
) -> anyhow::Result<R> {
    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("vr: the graphics device is unavailable")?;
    Ok(f(device))
}
