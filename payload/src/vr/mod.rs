//! The OpenXR runtime: session lifecycle, event pump, and the per-frame API surface the Draw wiring
//! drives. This module owns the OpenXR instance, session, reference spaces, and stereo swapchain. The
//! per-frame loop ([`update`] → [`frame_begin`] → [`FrameContext`] → per-eye blit → [`present_and_submit`])
//! is driven from `hooks::game::game_update_render`; the per-eye render parameters flow to the camera
//! hook through [`frame`]'s separate slot ([`render_params`]), not the frame-held runtime lock. See
//! `docs/mod/vr-runtime.md` for the loop end to end.
//!
//! ## Loader route
//!
//! The OpenXR loader is **dynamically loaded at runtime** (`xr::Entry::load_from`), not linked. The
//! `static` loader route (build the Khronos loader through cmake against the xwin/clang-cl cross
//! toolchain) does not build in this environment -- cmake selects the Ninja generator, which the
//! cross toolchain lacks -- so the portable choice is the runtime loader. The loader DLL defaults to
//! `openxr_loader.dll` next to the payload DLL ([`crate::module::get_path`]) and is overridable via
//! [`crate::vr::VrConfig::loader_path`]. When the loader is absent the mod stays in flatscreen
//! stereo and retries on the configured cadence.
//!
//! ## Threading
//!
//! Everything runs on the game's main thread, the same model as [`crate::capture`]: a single
//! `Mutex<VrState>` singleton, locked briefly on that thread. The game's `ID3D11Device` is fetched
//! from the graphics engine singleton at session-create time under the same null-guarding
//! [`crate::capture`] uses; the device is never stored (so the state carries no raw device pointer
//! across threads). All OpenXR handles are `Send` (`Arc`-backed handles), so the state is a safe
//! singleton.
//!
//! ## Degradation and retry
//!
//! Bring-up failure at any stage logs on target `"vr"` and leaves the mod in flatscreen stereo;
//! [`update`] retries the whole bring-up every [`crate::vr::VrConfig::retry_interval_secs`] while
//! `vr.enabled`. Turning `vr.enabled` off, or [`crate::lifecycle`] shutdown, tears the runtime down
//! in order (swapchain → session → instance) so the OpenXR instance never outlives the DLL.

pub mod projection;

pub use config::{
    BlitGamma, FoveationConfig, FoveationConfigError, FreezeMode, MirrorFraming,
    ProjectionConvention, VrConfig,
};
pub use frame::{
    EyeRenderParams, begin_render_frame, clear_render_params, cull_projection_standard,
    render_params,
};
use openxr as xr;
pub use projection::{Fov, OffAxisProjection};

mod back_buffer;
mod blit;
mod config;
mod eye_resolution;
pub mod foveation;
mod frame;
mod frame_loop;
mod loader;
mod mirror;
mod persist;
pub mod pose_control;
mod recenter;
mod resolution;
mod session;
mod state;
mod swapchain;
pub mod tail;
pub(crate) mod window;

pub use back_buffer::{
    owned as back_buffer_owned, resize_substitute_bypassed, substitute_render_setups,
    sync_swapchain_to_window,
};
pub use blit::present_and_submit;
pub use eye_resolution::{engine_render_resolution, native_eye_resolution};
pub use frame_loop::{EyeImage, EyeView, FrameContext, frame_begin};
pub use mirror::{MIRROR_ZOOM_RANGE, present_mirror};
pub use recenter::{auto_recenter_tick, recenter};
pub use resolution::apply_native_resolution;
pub use state::{VrStatus, install, is_running, status, uninstall, update};

/// The OpenXR view configuration: standard stereo, two views (one per eye).
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
/// The number of views (eyes), and the swapchain array size (one slice per eye).
const VIEW_COUNT: u32 = 2;
