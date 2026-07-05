//! Floating-HUD configuration types. See `docs/hud.md`.

use serde::{Deserialize, Serialize};

/// Floating-HUD settings. See `docs/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    /// Redirect the HUD into our own offscreen texture (the first step toward the floating panel).
    /// Off leaves the HUD on the engine surface as normal.
    pub redirect: bool,
    /// Draw the redirected HUD back into the scene as a floating quad, per eye. Requires `redirect`.
    pub quad: bool,
    /// Aspect ratio (width / height) for the gameplay HUD; `1.0` is square. The effective aspect for
    /// the current frame ([`hud_aspect`](HudConfig::hud_aspect) or [`movie_aspect`](HudConfig::movie_aspect),
    /// per the [`HudMode`](crate::hud::HudMode)) is the single source of truth for the HUD's shape:
    /// the render-target dimensions, the floating panel, the marker projection
    /// ([`compute_panel_vp`](crate::hud::compute_panel_vp)), and the Scaleform viewport all derive
    /// from it, so they cannot drift out of sync.
    pub hud_aspect: f32,
    /// Aspect ratio (width / height) for full-screen UI -- movies, loading screens, and menus
    /// ([`HudMode::Movie`](crate::hud::HudMode)); `16:9` by default. See [`hud_aspect`](HudConfig::hud_aspect).
    pub movie_aspect: f32,
    /// HUD render-target scale relative to the game's largest back-buffer axis. The texture's longer
    /// axis is `render_scale * max(back_buffer_width, back_buffer_height)` pixels; the shorter axis
    /// follows from the effective aspect. Lower trades sharpness for fill rate.
    pub render_scale: f32,
    /// Distance from the eye to the panel, in meters. The panel resizes with distance to keep a
    /// constant apparent (angular) size, so this can be changed freely without the HUD growing or
    /// shrinking. Comfort band: 1.5-3m.
    pub distance: f32,
    /// Apparent-size multiplier for the panel; `1.0` is the comfortable baseline (4 m wide at 3 m).
    /// The physical size is derived from this, [`distance`](HudConfig::distance), and the effective
    /// aspect (see [`crate::hud::panel_height`]), so changing the distance or aspect keeps the panel
    /// looking the same size and fitting the same horizontal content.
    pub panel_scale: f32,
    /// Lazy-follow damping parameters for the floating panel.
    pub follow: FollowConfig,
    /// Distance from the eye to the world-marker layer while splitting, in meters. Markers keep a
    /// constant apparent size (like the panel), so this only changes their stereo depth. The
    /// per-marker depth warp supersedes this as markers' effective depth when enabled.
    pub marker_distance: f32,
    /// Distance from the eye to the screen-center (reticle) layer while splitting, in meters.
    /// Constant apparent size; the fallback (and easing target) while
    /// [`center_depth_from_aim`](HudConfig::center_depth_from_aim) has no recent aim point.
    pub center_distance: f32,
    /// Drive the center layer's depth from the grapple reticle's aim point (smoothed), so the
    /// reticle group sits at the vergence of the surface it targets.
    pub center_depth_from_aim: bool,
    /// Warp the panel per element: each on-screen world marker's neighborhood is displaced to
    /// the marker's real world depth (recorded from the game's own world-to-screen calls), and
    /// the screen-center reticle region to the aim depth, giving depth-correct stereo disparity
    /// without re-rendering the HUD. Applies to the single panel; while
    /// [`split`](HudConfig::split) is on it applies to the marker layer instead.
    pub marker_warp: bool,
    /// The radius of the center (reticle) region displaced to the aim depth, in texture-uv units.
    pub center_bubble_radius: f32,
    /// The warp falloff radius around each marker, in texture-uv units.
    pub marker_radius: f32,
    /// Marker depths are clamped to this, in meters -- beyond it disparity is indistinguishable
    /// from infinity.
    pub marker_max_depth: f32,
    /// PARKED, off by default: split the HUD into three depth layers -- static HUD, world
    /// markers, reticles -- each in its own texture composited at its own depth, at full rate
    /// (the movie's render tree partitioned across extra render roots). Gameplay works, but the
    /// first pause permanently stops the UI update pump; see the post-mortem in
    /// `docs/issue-08-14-hud-overlays-and-depth.md` and `payload/src/hud/split/`.
    pub split: bool,
    /// Keep the full-screen Scaleform overlays -- drowning tint, damage flashes, directional
    /// damage indicators -- hidden (issue #8): they were authored to cover a flat screen and
    /// cover the whole panel in VR instead. Enforced per frame on the game thread through the
    /// discovered clip handles, ahead of each capture.
    pub suppress_overlays: bool,
    /// Dynamic panel distance from the scene depth distribution.
    pub depth_shift: DepthShiftConfig,
    /// The clip-path prefix from the root movie's timeline to the HUD movie's clips, ending in a
    /// dot when non-empty (e.g. `"hud."`). The HUD movie is attached by `root.gfx`'s ActionScript
    /// under a runtime name the display-tree dump reveals; until confirmed in-game, the authored
    /// clip names are tried bare.
    pub split_path_prefix: SplitPathPrefix,
}
impl HudConfig {
    pub const fn new() -> Self {
        Self {
            redirect: true,
            quad: true,
            hud_aspect: 1.0,
            movie_aspect: 16.0 / 9.0,
            render_scale: 1.0,
            distance: 3.0,
            panel_scale: 1.0,
            follow: FollowConfig::new(),
            marker_distance: 3.0,
            center_distance: 3.0,
            center_depth_from_aim: true,
            marker_warp: false,
            center_bubble_radius: 0.12,
            marker_radius: 0.08,
            marker_max_depth: 150.0,
            split: false,
            depth_shift: DepthShiftConfig::new(),
            suppress_overlays: true,
            split_path_prefix: SplitPathPrefix::new(),
        }
    }
}

/// Dynamic panel distance from the scene depth distribution (issue #14): a compute pass
/// histograms the whole main depth buffer each frame, and the panel eases toward
/// [`near_distance`](DepthShiftConfig::near_distance) while enough of the frame sits nearer than
/// [`near_threshold`](DepthShiftConfig::near_threshold) (a vehicle interior, a corridor, a
/// wall), back to the base [`distance`](HudConfig::distance) otherwise, and always far during
/// full-screen UI. See `payload/src/hud/depth.rs`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthShiftConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Depths nearer than this count as near-field, in meters.
    pub near_threshold: f32,
    /// The fraction of the frame that must be near-field to engage the near shift.
    pub near_occupancy: f32,
    /// The occupancy slack below the engage level before the shift releases (hysteresis, so the
    /// panel does not flap at the boundary).
    pub hysteresis: f32,
    /// The panel distance while the near shift is engaged, in meters.
    pub near_distance: f32,
    /// The easing halflife between distances, in seconds.
    pub halflife: f32,
    /// EXPERIMENTAL: follow the scene continuously instead of the threshold policy -- the panel
    /// sits [`margin`](DepthShiftConfig::margin) inside the configured percentile of the depth
    /// distribution, clamped to [`near_distance`](DepthShiftConfig::near_distance) and the base
    /// distance.
    pub continuous: bool,
    /// The depth percentile the continuous policy follows (0-1).
    pub percentile: f32,
    /// How far inside the percentile depth the continuous policy sits, in meters.
    pub margin: f32,
    /// Sample every Nth pixel of the depth buffer on both axes.
    pub sample_stride: u32,
}

impl DepthShiftConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            near_threshold: 2.0,
            near_occupancy: 0.2,
            hysteresis: 0.05,
            near_distance: 1.1,
            halflife: 0.35,
            continuous: false,
            percentile: 0.10,
            margin: 0.3,
            sample_stride: 4,
        }
    }
}

impl Default for DepthShiftConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A short fixed-capacity string for [`HudConfig::split_path_prefix`], so `HudConfig` stays `Copy`
/// (the config is snapshotted by value throughout the payload).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SplitPathPrefix {
    bytes: [u8; Self::CAPACITY],
    len: u8,
}

impl SplitPathPrefix {
    /// Enough for a couple of nesting levels of clip names.
    pub const CAPACITY: usize = 64;

    pub const fn new() -> Self {
        Self {
            bytes: [0; Self::CAPACITY],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // The only writers validated UTF-8 on the way in.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }

    /// Replace the prefix. Fails when the string exceeds the capacity.
    pub fn set(&mut self, value: &str) -> Result<(), PrefixTooLongError> {
        let bytes = value.as_bytes();
        if bytes.len() > Self::CAPACITY {
            return Err(PrefixTooLongError { len: bytes.len() });
        }
        self.bytes = [0; Self::CAPACITY];
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len() as u8;
        Ok(())
    }
}

impl TryFrom<String> for SplitPathPrefix {
    type Error = PrefixTooLongError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut prefix = Self::new();
        prefix.set(&value)?;
        Ok(prefix)
    }
}

impl From<SplitPathPrefix> for String {
    fn from(value: SplitPathPrefix) -> Self {
        value.as_str().to_string()
    }
}

/// The clip-path prefix exceeds [`SplitPathPrefix::CAPACITY`].
#[derive(Debug)]
pub struct PrefixTooLongError {
    len: usize,
}

impl std::fmt::Display for PrefixTooLongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hud config: the split path prefix is {} bytes; the capacity is {}",
            self.len,
            SplitPathPrefix::CAPACITY
        )
    }
}

impl std::error::Error for PrefixTooLongError {}

/// Lazy-follow damping parameters for the floating HUD panel. See `docs/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FollowConfig {
    /// Rotation follow halflife in seconds. Lower = snappier follow.
    pub rotation_halflife: f32,
    /// Position de-jitter halflife in seconds.
    pub position_halflife: f32,
}
impl FollowConfig {
    pub const fn new() -> Self {
        Self {
            rotation_halflife: 0.2,
            position_halflife: 0.1,
        }
    }
}
