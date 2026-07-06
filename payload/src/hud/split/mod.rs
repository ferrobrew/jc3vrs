//! The parked per-element depth split: layer definitions and the render-root partition.
//!
//! Disabled by default. Gameplay works (full-rate per-layer textures via the render-root
//! partition, composited at per-layer depths), but the first pause permanently stops the UI
//! update pump -- see the post-mortem in `docs/issues/08-14-hud-overlays-and-depth.md` for the
//! complete history of the three mechanisms tried, the constraints they established, and the
//! diagnostic next step if this is revived. Nothing outside this module (and its gated call
//! sites) depends on the split; the clip-handle registry it uses lives with the Scaleform
//! plumbing in [`super::scaleform`], because overlay suppression (issue #8) shares it and ships
//! independently.

pub mod roots;

use windows::Win32::Graphics::Direct3D11::{ID3D11DepthStencilView, ID3D11RenderTargetView};

pub use super::scaleform::LAYER_COUNT;

/// The HUD layers, in composite order (bottom to top). Each is one texture; the render-root
/// partition redraws every layer every frame.
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

/// The per-layer render-target views, snapshotted from the HUD state before the render detour
/// uses them (so the state lock is not held across the original render). The views are COM
/// clones, so they keep their textures alive even if the state recreates its targets concurrently.
pub struct LayerViews {
    /// `(RTV, DSV)` per layer, in [`HudLayer`] order. Layer 0 (static) is the main HUD target.
    pub(super) views: [(ID3D11RenderTargetView, ID3D11DepthStencilView); LAYER_COUNT],
    /// Each layer texture's `(width, height)`.
    pub(super) sizes: [(u32, u32); LAYER_COUNT],
}
