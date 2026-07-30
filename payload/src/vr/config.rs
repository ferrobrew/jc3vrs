//! VR runtime configuration. See [`crate::vr`] and `docs/mod/vr-runtime.md`.

use jc3gi::graphics_engine::render_engine::RenderPassId;
use serde::{Deserialize, Serialize};

/// Serialize a [`RenderPassId`] as its `i32` discriminant, matching the pre-typing on-disk format
/// (`0x41` etc.) so existing config files stay readable.
fn serialize_render_pass_id<S: serde::Serializer>(
    pass: &RenderPassId,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_i32(*pass as i32)
}

/// Deserialize a [`RenderPassId`] from an `i32`, accepting any value that falls within the enum's
/// range. The config file stores the integer; an out-of-range value is a deserialization error.
fn deserialize_render_pass_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<RenderPassId, D::Error> {
    let raw = i32::deserialize(deserializer)?;
    render_pass_id_from_i32(raw).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "render pass id {raw:#x} is out of the RenderPassId range"
        ))
    })
}

/// Map an `i32` to the matching [`RenderPassId`] variant, or `None` if it is outside the enum range.
///
/// `RenderPassId` is `#[repr(i32)]` with contiguous discriminants `0..=157`
/// (`RP_NONE` through `RP_RENDERPASS_COUNT`), so every value in that range is a valid variant.
fn render_pass_id_from_i32(raw: i32) -> Option<RenderPassId> {
    if (0..=157).contains(&raw) {
        // SAFETY: `RenderPassId` is `#[repr(i32)]` and every value in `0..=157` is a named variant,
        // so transmuting an in-range `i32` is sound.
        Some(unsafe { std::mem::transmute::<i32, RenderPassId>(raw) })
    } else {
        None
    }
}

/// Which depth convention the per-eye off-axis projection is written in, and where in the
/// `SetupRenderCamera` sequence it lands (`docs/engine/rendering/rendering.md` §2.7, `docs/mod/vr-runtime.md` blocker 1).
/// The coordinate/depth conventions are the least-verifiable part of the pipeline without a headset,
/// so this is a runtime tweakable rather than a compile-time choice.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectionConvention {
    /// **Preferred and verified-correct.** Write a standard (non-reverse-Z) off-axis projection into
    /// `m_Projection` *before* the engine's `SetupRenderCamera`, so the engine applies its reverse-Z
    /// remap and TAA jitter to it exactly once, matching every other camera. `SetupRenderCamera`
    /// consumes the pre-written `m_Projection` in place rather than rebuilding it from FOV/near/far
    /// (settled against the engine, `docs/engine/rendering/rendering.md` §2.9), so this write reaches the GPU.
    #[default]
    EnginePreReverseZ,
    /// **Fallback / escape hatch.** Write an already-reverse-Z'd off-axis projection *after*
    /// `SetupRenderCamera` (so the engine does not re-reverse it), then rebuild the view-projections
    /// manually. TAA jitter is not applied on this path.
    ///
    /// The consume-vs-rebuild question this guarded against is now settled against the engine
    /// (`docs/engine/rendering/rendering.md` §2.9): `Camera::SetupRenderCamera` *consumes* whatever is in
    /// `m_Projection`, applying `z' = w - z` to it in place — it never rebuilds from FOV/near/far —
    /// so the pre-call [`EnginePreReverseZ`](Self::EnginePreReverseZ) write flows through correctly
    /// and is the verified-correct default. This variant is retained only as a runtime escape hatch
    /// for a headset playtest, in case the depth still reads wrong for a reason not visible from the
    /// desktop.
    ManualReverseZ,
}

/// Which pose the freeze diagnostic holds still (see [`crate::vr::pose_control`]). The two frozen
/// modes are two answers to the same question -- "what do you want held still?" -- so they are
/// mutually exclusive: freezing the head's contribution and freezing the whole camera are not
/// composable, and holding both would only mean the second one.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum FreezeMode {
    /// Nothing is frozen; the camera follows the head, the body, and the game as usual.
    #[default]
    Off,
    /// Hold the **cockpit-frame HMD pose**: the head pose relative to the recenter baseline, plus the
    /// sim-driven body frame and head-bone anchor captured with it. The rendered camera is then
    /// bit-identical frame to frame *as long as nothing else moves it*, which isolates artifacts
    /// driven by the HMD's per-frame pose sensor noise (present even sitting on a desk) from ones
    /// intrinsic to the render. The body frame and anchor are captured too, so on-foot idle motion
    /// does not leak in; a camera the *game* moves (a vehicle, a cutscene) still does.
    CockpitPose,
    /// Hold the **final render camera** in world space: the scene camera's world transform is pinned
    /// at the last point before the engine consumes it, so nothing upstream -- the head, the body, the
    /// animated head-bone anchor, or the game's own camera -- can move the view. The per-eye offsets
    /// and projections are held with it, so the two eyes are static too. This is the mode for
    /// measuring content that mis-renders only *in motion*: the view is a fixed world-space pose that
    /// can be stepped by an exact amount and returned to.
    FullCamera,
}

/// How the per-eye blit bridges the game's captured back-buffer colour into the OpenXR swapchain.
///
/// The captured eye texture is a `CopyResource` of `m_BackBufferLinear` as `R8G8B8A8_UNORM`
/// (non-sRGB); the game presents those same bytes to a non-sRGB desktop swapchain and they look
/// correct, so the stored bytes are **display-referred** (already sRGB-encoded). The negotiated
/// OpenXR swapchain is `_SRGB`, so writing through its render-target view applies a hardware
/// linear→sRGB encode. To reproduce the original bytes the shader must therefore **linearize** the
/// sampled colour first, so the hardware re-encode cancels it out ([`Linearize`](Self::Linearize),
/// the default). If the swapchain ends up non-sRGB, or the captured content turns out to be genuine
/// linear despite the copy, [`Passthrough`](Self::Passthrough) samples and writes the colour
/// unchanged. Colours cannot be eyeballed without a headset, so this stays switchable at runtime.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlitGamma {
    /// Decode the sampled display-referred colour to linear before writing, so the `_SRGB`
    /// render-target's hardware encode reproduces the original bytes. The correct default for a
    /// display-referred source into an `_SRGB` target.
    #[default]
    Linearize,
    /// Sample and write the colour unchanged. Correct for a genuine-linear source, or a non-sRGB
    /// target that applies no encode.
    Passthrough,
}

/// How the desktop mirror frames one eye's image inside the game window.
///
/// While a session runs the swapchain buffers are the per-eye render resolution, which is near-square
/// (often taller than wide), while the Win32 client rect keeps its original widescreen aspect. The two
/// aspects have to be reconciled, and the choice is the same one every VR game's companion view makes.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MirrorFraming {
    /// Scale the eye image up until it covers the whole window, cropping whatever falls outside
    /// (centred). No bars; the periphery of the eye render -- most of which is outside the lens's
    /// useful area anyway -- is cut off. This is what other VR titles' desktop views do, and the
    /// default.
    #[default]
    Fill,
    /// Scale the eye image down until all of it is visible, letterboxing the remainder with black
    /// bars. Nothing is cropped, so it shows exactly what the eye rendered -- useful when checking
    /// coverage at the edges of the frame -- but a near-square eye in a widescreen window wastes most
    /// of the window.
    Fit,
}

/// OpenXR runtime settings. `Clone` (not `Copy`) because [`loader_path`](VrConfig::loader_path) owns
/// a `String`.
#[derive(Clone, Serialize, Deserialize)]
pub struct VrConfig {
    /// Master switch: bring up the OpenXR session and render to the HMD. Off leaves the mod in
    /// flatscreen stereo (and tears any live runtime down). While on, bring-up is retried on the
    /// [`retry_interval_secs`](VrConfig::retry_interval_secs) cadence until it succeeds.
    pub enabled: bool,
    /// Per-eye swapchain resolution scale, applied to the runtime's recommended width/height. `1.0`
    /// is the runtime's recommendation; lower trades sharpness for fill rate. Clamped to a small
    /// positive minimum at swapchain creation.
    pub resolution_scale: f32,
    /// How often, in seconds, to retry OpenXR bring-up after a failure while
    /// [`enabled`](VrConfig::enabled). The mod runs in flatscreen stereo between attempts.
    pub retry_interval_secs: u64,
    /// World scale: metres of head/IPD motion per engine unit (`1.0` = 1:1). Kept here so the render wiring and the camera path share one knob.
    pub world_scale: f32,
    /// Override path to the OpenXR loader DLL. `None` loads `openxr_loader.dll` next to the payload
    /// DLL, falling back to the platform default search path.
    pub loader_path: Option<String>,
    /// Fallback near clip plane, in metres, for the per-eye off-axis projection, used only until the
    /// first camera update publishes the engine's live plane. The mod reads the active camera's actual
    /// `m_Near` each frame as the source of truth (see
    /// [`crate::hooks::camera::main_camera_planes_or`]); this default (`0.1`) mirrors the engine's
    /// `Camera` constructor value (`docs/engine/rendering/rendering.md` §2.9) so the bootstrap frame matches.
    pub near_clip: f32,
    /// Fallback far clip plane, in metres, for the per-eye off-axis projection, used only until the
    /// first camera update publishes the engine's live plane. The mod reads the active camera's actual
    /// `m_Far` each frame as the source of truth (see
    /// [`crate::hooks::camera::main_camera_planes_or`]) — the game renders a finite-far reverse-Z
    /// frustum and sets its own runtime far, so matching the live value keeps the eyes, the cull
    /// frustum, and the depth reconstruction consistent and the horizon unclipped. This default
    /// (`38400`) mirrors the engine's `Camera` constructor value (`0x47160000`) for the bootstrap frame.
    pub far_clip: f32,
    /// Which depth convention the per-eye off-axis projection is written in (see
    /// [`ProjectionConvention`]). Defaults to the preferred pre-`SetupRenderCamera` write.
    #[serde(default)]
    pub projection_convention: ProjectionConvention,
    /// How the per-eye blit bridges the captured colour into the `_SRGB` swapchain (see
    /// [`BlitGamma`]). Defaults to linearizing the display-referred capture.
    #[serde(default)]
    pub blit_srgb_gamma: BlitGamma,
    /// Render each eye at the HMD-recommended per-eye resolution (× [`resolution_scale`]) rather than
    /// the desktop display size, by driving the engine's own deferred resize (see
    /// [`crate::vr::resolution`]). On by default; disabled automatically at runtime if the resize
    /// path faults or returns the wrong size, falling back to the desktop resolution.
    ///
    /// [`resolution_scale`]: VrConfig::resolution_scale
    #[serde(default = "default_true")]
    pub native_resolution: bool,
    /// Mirror one eye to the game's own desktop window while a session is running. The engine's
    /// present stays blocked (`BLOCK_FLIP`); the mirror draws the configured eye's capture into the
    /// game swapchain's back buffer, framed to the window aspect, and presents it unsynced (see
    /// [`crate::vr::mirror`]). On by default; disabled automatically at runtime on any draw/present
    /// fault, after which the game window simply shows the last mirrored (or stale) frame.
    #[serde(default = "default_true")]
    pub mirror: bool,
    /// Which eye the desktop [`mirror`](VrConfig::mirror) shows (`0` = left, `1` = right). Clamped to
    /// a valid eye at use.
    #[serde(default)]
    pub mirror_eye: u8,
    /// How the desktop [`mirror`](VrConfig::mirror) reconciles the eye image's aspect with the window's
    /// (see [`MirrorFraming`]). Defaults to filling the window.
    #[serde(default)]
    pub mirror_framing: MirrorFraming,
    /// Extra magnification applied to the desktop [`mirror`](VrConfig::mirror) image on top of its
    /// framing, about the centre. `1.0` is the plain fill/fit; above that crops in further, which
    /// tightens the desktop view onto the middle of the eye render (the eye covers a much wider field
    /// of view than a flat game would frame). Clamped to a sane range at use; a non-finite value falls
    /// back to `1.0`.
    #[serde(default = "default_mirror_zoom")]
    pub mirror_zoom: f32,
    /// Persist the OpenXR **instance and session** across inject/uninject cycles instead of destroying
    /// them on teardown. The runtime allows only a small number of instances *and* sessions per
    /// process (often one each), and Proton's own startup VR probe contends for that budget, so a
    /// reinject that creates fresh ones fails with `XR_ERROR_LIMIT_REACHED`. With this on, teardown
    /// stashes both handles in the game process's environment and leaks the wrappers (the handles stay
    /// valid for the process lifetime), *without* ending the session — an ended session cannot be
    /// resumed — and a reinject re-wraps both rather than creating new ones, so VR comes back on
    /// reinject without a game relaunch. The swapchain and reference space are recreated on the reused
    /// session. On by default; a stale handle falls back to a fresh create, and a genuine stop
    /// (`enabled` off, or a lost session) destroys everything and clears the stashes.
    #[serde(default = "default_true")]
    pub persist_instance: bool,
    /// Recenter automatically when gameplay control returns to the player. Injecting VR before the
    /// scripted resume-from-menu animation (Rico standing up from the car) leaves the rig at an
    /// offset from the camera, and a fresh session's neutral is wherever the head was at session
    /// start rather than where the player actually is. With this on, the mod arms while the game is in
    /// the frontend / loading / has no local player, and fires a single [`recenter`](crate::vr::recenter)
    /// once gameplay is running and the player's head has settled (the entry animation has finished),
    /// so the neutral snaps to the player's real pose without a manual F7. It does not fire on
    /// in-session transitions like exiting a vehicle (the character stays present through those).
    #[serde(default = "default_true")]
    pub auto_recenter_on_gameplay: bool,
    /// Substitute a mod-owned render target for the engine's swapchain-derived back buffer, so the
    /// render resolution and the DXGI swapchain size stop being the same number (see
    /// [`crate::vr::back_buffer`] and `docs/mod/stereo/swapchain-ownership.md`).
    ///
    /// The engine builds its final composite target as a format alias of DXGI back buffer 0, so
    /// driving the scene to the per-eye render resolution resizes the swapchain with it, and the
    /// desktop present then rescales every frame onto a window of a different size and shape. Owning
    /// the back buffer breaks that link: the swapchain stays at the window size, the mirror's present
    /// stops rescaling, and single-pass double-wide stops forcing a 2x-width swapchain.
    ///
    /// On by default; it only ever engages while an XR session is running, and is released again
    /// (with the engine's own objects rebuilt over the live swapchain) on session end and on eject.
    #[serde(default = "default_true")]
    pub own_back_buffer: bool,
    /// Diagnostic: which pose to hold still (see [`FreezeMode`] and [`crate::vr::pose_control`]).
    /// Whichever pose the mode holds is captured on the first frame it is on and reused every frame
    /// after, and is hand-editable while held -- so a motion-dependent artifact can be driven by an
    /// exact, repeatable step instead of an eyeballed head movement. Not for gameplay: the view locks
    /// in place. **Default off.**
    #[serde(default)]
    pub freeze_mode: FreezeMode,
    /// The translation step, in metres, of one nudge of the frozen pose's position (see
    /// [`crate::vr::pose_control`]). Nudges snap to this grid, so a step is exactly repeatable.
    #[serde(default = "default_freeze_pose_step_m")]
    pub freeze_pose_step_m: f32,
    /// The rotation step, in degrees, of one nudge of the frozen pose's yaw/pitch/roll (see
    /// [`crate::vr::pose_control`]). Nudges snap to this grid, so a step is exactly repeatable.
    #[serde(default = "default_freeze_pose_step_deg")]
    pub freeze_pose_step_deg: f32,
}

/// The serde default for [`VrConfig::native_resolution`] (the manual [`Default`] via
/// [`VrConfig::new`] is not consulted per-field when a field is absent from the serialized form).
fn default_true() -> bool {
    true
}

/// The serde default for [`VrConfig::mirror_zoom`] (see [`default_true`] for why this is needed).
fn default_mirror_zoom() -> f32 {
    1.0
}

/// The serde default for [`VrConfig::freeze_pose_step_m`] (see [`default_true`] for why this is needed).
fn default_freeze_pose_step_m() -> f32 {
    0.1
}

/// The serde default for [`VrConfig::freeze_pose_step_deg`] (see [`default_true`] for why this is needed).
fn default_freeze_pose_step_deg() -> f32 {
    1.0
}

impl VrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            resolution_scale: 1.0,
            retry_interval_secs: 10,
            world_scale: 1.0,
            loader_path: None,
            near_clip: 0.1,
            far_clip: 38400.0,
            projection_convention: ProjectionConvention::EnginePreReverseZ,
            blit_srgb_gamma: BlitGamma::Linearize,
            native_resolution: true,
            mirror: true,
            mirror_eye: 0,
            mirror_framing: MirrorFraming::Fill,
            mirror_zoom: 1.0,
            persist_instance: true,
            auto_recenter_on_gameplay: true,
            own_back_buffer: true,
            freeze_mode: FreezeMode::Off,
            freeze_pose_step_m: 0.1,
            freeze_pose_step_deg: 1.0,
        }
    }
}

impl Default for VrConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Static foveated rendering (issue #29): a radial stencil mask drops a dithered fraction of the
/// peripheral pixels before the expensive scene passes shade them, and a fill-in pass reconstructs them
/// from their neighbours. On by default (validated in-headset); costs the mask + fill passes but saves
/// peripheral shading. See `docs/mod/stereo/foveation.md`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FoveationConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Foveal radius as a fraction of the half-diagonal from the per-eye foveal centre: inside this the
    /// scene renders at full rate (no pixels dropped).
    pub inner_fraction: f32,
    /// Radius (same units as [`inner_fraction`](Self::inner_fraction)) at which the peripheral drop
    /// reaches [`max_drop`](Self::max_drop); the drop ramps from zero at the inner radius to the max here.
    pub outer_fraction: f32,
    /// The maximum fraction of peripheral pixels dropped (and reconstructed), reached at
    /// [`outer_fraction`](Self::outer_fraction) and beyond. `0.5` drops half the far periphery.
    pub max_drop: f32,
    /// The stencil bit the mask tags dropped pixels with. Default `0x80` (bit 7): the engine's own
    /// stencil use is bits 0, 1, 5, 6, so bit 7 is free; change it if an effect corrupts with foveation
    /// on (a data-driven pass may write it).
    pub mask_bit: u32,
    /// The first [`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId) (inclusive) of the
    /// foveated shading range: the mask-write runs just before it, and the peripheral stencil test is
    /// forced on from here. Default `RP_MODELS_DYNAMIC` -- after the depth prepass, so the
    /// dropped pixels keep full-resolution depth. A tuning knob; widen it toward the lighting passes to
    /// save more, narrow it if an effect misbehaves.
    #[serde(
        serialize_with = "serialize_render_pass_id",
        deserialize_with = "deserialize_render_pass_id"
    )]
    pub foveal_first_pass: RenderPassId,
    /// The last [`RenderPassId`](jc3gi::graphics_engine::render_engine::RenderPassId) (inclusive) of the
    /// foveated shading range: the peripheral stencil test is forced through it, and the fill-in runs just
    /// after. Default `RP_CREATURES`.
    #[serde(
        serialize_with = "serialize_render_pass_id",
        deserialize_with = "deserialize_render_pass_id"
    )]
    pub foveal_last_pass: RenderPassId,
    /// Diagnostic: paint the dropped peripheral pixels magenta (in the fill-in pass) instead of
    /// reconstructing them, so the mask is directly visible. Off by default.
    pub debug_show_mask: bool,
}
impl FoveationConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            inner_fraction: 0.35,
            outer_fraction: 0.55,
            max_drop: 0.8,
            mask_bit: 0x80,
            foveal_first_pass: RenderPassId::RP_MODELS_DYNAMIC,
            foveal_last_pass: RenderPassId::RP_CREATURES,
            debug_show_mask: false,
        }
    }

    /// Validate the foveation configuration: the first pass must not exceed the last, and the mask
    /// bit must be a single set bit (a power of two). Returns `Err` with a description of the first
    /// problem found.
    pub fn validate(&self) -> Result<(), FoveationConfigError> {
        if self.foveal_first_pass > self.foveal_last_pass {
            return Err(FoveationConfigError::PassRange {
                first: self.foveal_first_pass as i32,
                last: self.foveal_last_pass as i32,
            });
        }
        if self.mask_bit == 0 || (self.mask_bit & (self.mask_bit - 1)) != 0 {
            return Err(FoveationConfigError::MaskNotPowerOfTwo {
                mask_bit: self.mask_bit,
            });
        }
        Ok(())
    }
}
impl Default for FoveationConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A configuration error found by [`FoveationConfig::validate`].
#[derive(Debug)]
pub enum FoveationConfigError {
    /// The foveal pass range is inverted (`first > last`).
    PassRange { first: i32, last: i32 },
    /// The stencil mask bit is zero or not a single set bit.
    MaskNotPowerOfTwo { mask_bit: u32 },
}

impl std::fmt::Display for FoveationConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FoveationConfigError::PassRange { first, last } => write!(
                f,
                "foveation: foveal_first_pass ({first:#x}) is greater than foveal_last_pass ({last:#x})"
            ),
            FoveationConfigError::MaskNotPowerOfTwo { mask_bit } => write!(
                f,
                "foveation: mask_bit ({mask_bit:#x}) must be a single set bit (a power of two)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foveation_config_validate_accepts_defaults() {
        let cfg = FoveationConfig::new();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn foveation_config_validate_rejects_inverted_range() {
        let mut cfg = FoveationConfig::new();
        cfg.foveal_first_pass = RenderPassId::RP_CREATURES;
        cfg.foveal_last_pass = RenderPassId::RP_MODELS_DYNAMIC;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn foveation_config_validate_rejects_zero_mask_bit() {
        let mut cfg = FoveationConfig::new();
        cfg.mask_bit = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn foveation_config_validate_rejects_non_power_of_two_mask_bit() {
        let mut cfg = FoveationConfig::new();
        cfg.mask_bit = 0x60;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn foveation_config_validate_accepts_equal_range() {
        let mut cfg = FoveationConfig::new();
        cfg.foveal_first_pass = RenderPassId::RP_CREATURES;
        cfg.foveal_last_pass = RenderPassId::RP_CREATURES;
        assert!(cfg.validate().is_ok());
    }
}
