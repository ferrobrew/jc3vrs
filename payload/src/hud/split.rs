//! The HUD split's Scaleform-side registry: the layer partition of `hud.gfx`'s containers, the
//! issue #8 overlay clips, and the pinned clip-handle registry the capture-seam hooks operate
//! through (the overlay suppression here, the render-root partition in [`super::roots`]).
//!
//! Two split mechanisms preceded the render-root partition and are documented in
//! `docs/issue-08-14-hud-overlays-and-depth.md`: multi-pass visibility rendering (structurally
//! unsound: captures are only safe on the game update thread) and time multiplexing (stable but
//! visibly undersampled: world-to-screen'd content needs full-rate sampling).

use jc3gi::ui::scaleform::{DisplayInfo, Value};
use windows::Win32::Graphics::Direct3D11::{ID3D11DepthStencilView, ID3D11RenderTargetView};

/// The HUD layers, in composite order (bottom to top). Each is one texture, refreshed round-robin
/// one layer per frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudLayer {
    /// The static HUD: the corner/edge safe-area containers and the weapon-selection wheel. No
    /// world anchor; lives at the panel depth.
    Static = 0,
    /// World-anchored markers: the POI stage and the target tracker / score container.
    Markers = 1,
    /// The screen-center group: weapon/grapple/mech reticles, pickups, and center indicators.
    /// Composited on top, at the aim depth when that is driven.
    Center = 2,
}

/// The number of layers / textures.
pub const LAYER_COUNT: usize = 3;

/// The authored top-level containers of `hud.gfx` assigned to each layer. Paths are relative to
/// the HUD movie's timeline; [`HudConfig::split_path_prefix`](crate::hud::HudConfig) is prepended
/// at call time once the runtime attachment point is known.
pub(crate) const LAYER_CONTAINERS: [&[&str]; LAYER_COUNT] = [
    // Static: the six corner/edge safe-area groups and the selection wheel.
    &[
        "MCI_safe_area_top_left",
        "MCI_safe_area_top_middle",
        "MCI_safe_area_top_right",
        "MCI_safe_area_bottom_left",
        "MCI_safe_area_bottom_middle",
        "MCI_safe_area_bottom_right",
        "MCI_weapon_selection_wheel",
    ],
    // Markers: the POI stage, and MCI_hud (the target-tracker stage plus score/ghost data).
    &["MCI_poi_stage", "MCI_hud"],
    // Center: the whole screen-center group (reticles and center indicators).
    &["MCI_safe_area_center"],
];

/// The full-screen overlay clips of issue #8, all children of the center container: held
/// invisible while [`HudConfig::suppress_overlays`](crate::hud::HudConfig::suppress_overlays) is
/// on. Paths confirmed against an in-game display-tree dump; `MCI_omni_damage` (the screen-wide
/// damage flash) is a direct child of the center container, not of the health-damage manager.
pub(crate) const OVERLAY_CLIPS: &[&str] = &[
    "MCI_safe_area_center.MCI_omni_damage",
    "MCI_safe_area_center.MCI_drowning_container",
    "MCI_safe_area_center.MCI_health_damage_manager",
    "MCI_safe_area_center.MCI_character_damage_indicators",
    "MCI_safe_area_center.MCI_vehicle_damage_indicators",
    "MCI_safe_area_center.MCI_inflict_damage",
    "MCI_safe_area_center.MCI_sniper",
];

/// A heap-pinned managed [`Value`] handle to a clip, plus the game-intent tracking the mask needs.
/// The `Value` is an intrusive list node on the movie's external-references list, so it must never
/// move after `GetVariable` fills it -- hence the box. Release (on the capture thread) before
/// dropping.
pub(crate) struct ClipHandle {
    /// The pinned managed value. Present until released; absent when the clip path did not
    /// resolve at discovery (the handle then reads as visible and writes are no-ops).
    pub value: Option<Box<Value>>,
    /// The game's own visibility intent for this clip, tracked across our forced writes: refreshed
    /// from a read-back whenever the current value differs from what we last wrote (meaning the
    /// game wrote in between).
    pub game_visible: bool,
    /// The visibility we last forced, or `None` while unforced.
    pub forced: Option<bool>,
}

impl ClipHandle {
    /// Wrap a resolved (or unresolved) pinned value.
    pub fn new(value: Option<Box<Value>>) -> Self {
        Self {
            value,
            game_visible: true,
            forced: None,
        }
    }

    /// Release the managed value through its object interface. Must run on the capture thread
    /// (the game update thread).
    pub unsafe fn release(&mut self) {
        if let Some(mut value) = self.value.take() {
            // SAFETY: the value was filled in place by GetVariable and never moved; managed
            // values carry their object interface.
            unsafe {
                if let Some(interface) = value.pObjectInterface.as_mut() {
                    let data = value.mValue as *mut std::ffi::c_void;
                    interface.ObjectRelease(value.as_mut(), data);
                }
            }
        }
    }
}

/// The clip handles the mask toggles, grouped by role. Built by the layout discovery on the game
/// thread; masked on the game thread; replaced wholesale on rediscovery (old handles released
/// first).
pub(crate) struct ClipHandles {
    /// Per layer, the named containers (in [`LAYER_CONTAINERS`] order, missing clips skipped).
    pub containers: [Vec<ClipHandle>; LAYER_COUNT],
    /// The issue #8 overlay clips.
    pub overlays: Vec<ClipHandle>,
    /// The anonymous POI pool (markers layer).
    pub dynamic: Vec<ClipHandle>,
}

// SAFETY: the raw pointers inside the handles (the pinned Values and their object interfaces)
// are only dereferenced on the capture (game update) thread, never concurrently -- the registry
// mutex plus that thread discipline provide the synchronization the pointer types cannot express.
unsafe impl Send for ClipHandle {}

/// The live handle registry. `None` until a discovery succeeds.
pub(crate) static CLIP_HANDLES: parking_lot::Mutex<Option<ClipHandles>> =
    parking_lot::Mutex::new(None);

/// The per-layer render-target views, snapshotted from the HUD state before the render detour
/// uses them (so the state lock is not held across the original render). The views are COM
/// clones, so they keep their textures alive even if the state recreates its targets concurrently.
pub struct LayerViews {
    /// `(RTV, DSV)` per layer, in [`HudLayer`] order. Layer 0 (static) is the main HUD target.
    pub(super) views: [(ID3D11RenderTargetView, ID3D11DepthStencilView); LAYER_COUNT],
    /// Each layer texture's `(width, height)`.
    pub(super) sizes: [(u32, u32); LAYER_COUNT],
}

/// Hold the issue #8 overlay clips hidden while `suppress` is on, tracking the game's own
/// visibility intent across our writes and restoring it on the off-transition. Runs at the
/// capture seam (game update thread, deferred render lock held).
///
/// # Safety
/// Must be called from the [`MovieImpl::Capture`](jc3gi::ui::scaleform::MovieImpl) detour.
pub unsafe fn apply_overlay_suppression(suppress: bool) {
    let mut handles = CLIP_HANDLES.lock();
    let Some(handles) = handles.as_mut() else {
        return;
    };
    // SAFETY: capture-seam threading per the function contract; the handles are pinned managed
    // values from the registry.
    unsafe {
        for handle in &mut handles.overlays {
            if suppress {
                refresh_intent(handle);
                force_visible(handle, false);
            } else if handle.forced.is_some() {
                refresh_intent(handle);
                unforce_visible(handle);
            }
        }
    }
}

/// Re-read a clip's game intent: when the current value differs from what we last wrote (or we
/// never wrote), the game changed it in between, so the read is its intent. Returns the intent.
unsafe fn refresh_intent(handle: &mut ClipHandle) -> bool {
    // SAFETY: forwarded to get_visible; see the caller's obligations.
    let read = unsafe { get_visible(handle) };
    if handle.forced != Some(read) {
        handle.game_visible = read;
    }
    handle.game_visible
}

/// Force a clip's visibility, remembering the write for the intent tracking.
unsafe fn force_visible(handle: &mut ClipHandle, visible: bool) {
    // SAFETY: forwarded to set_visible; see the caller's obligations.
    unsafe { set_visible(handle, visible) };
    handle.forced = Some(visible);
}

/// Restore a clip to the game's intent and stop tracking it as forced.
pub(crate) unsafe fn unforce_visible(handle: &mut ClipHandle) {
    if handle.forced.take().is_some() {
        let intent = handle.game_visible;
        // SAFETY: forwarded to set_visible; see the caller's obligations.
        unsafe { set_visible(handle, intent) };
    }
}

/// Read a clip's visibility through its cached handle, defaulting to visible on failure.
///
/// # Safety
/// The caller must be on the capture (game update) thread (see [`ClipHandle`]).
pub(crate) unsafe fn get_visible(handle: &mut ClipHandle) -> bool {
    // SAFETY: the handle's value is pinned and managed; the interface pointer comes from it.
    unsafe {
        let Some(value) = handle.value.as_mut() else {
            return true;
        };
        let Some(interface) = value.pObjectInterface.as_mut() else {
            return true;
        };
        let mut info = DisplayInfo::default();
        let data = value.mValue as *mut std::ffi::c_void;
        if !interface.GetDisplayInfo(data, &mut info) {
            return true;
        }
        info.Visible
    }
}

/// Write a clip's visibility through its cached handle (a `VarsSet = V_VISIBLE` display-info
/// write: no AVM, no AS3 setters).
///
/// # Safety
/// The caller must be on the capture (game update) thread (see [`ClipHandle`]).
pub(crate) unsafe fn set_visible(handle: &mut ClipHandle, visible: bool) {
    // SAFETY: as `get_visible`.
    unsafe {
        let Some(value) = handle.value.as_mut() else {
            return;
        };
        let Some(interface) = value.pObjectInterface.as_mut() else {
            return;
        };
        let mut info = DisplayInfo::default();
        info.VarsSet = DisplayInfo::V_VISIBLE as u16;
        info.Visible = visible;
        let data = value.mValue as *mut std::ffi::c_void;
        interface.SetDisplayInfo(data, &info);
    }
}
