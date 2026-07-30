//! The render resolution the engine is driven to while a session runs: the runtime's recommended
//! per-eye view size scaled and tile-aligned, and the double-wide widening the single-pass collapse
//! needs. Shared by the stereo swapchain ([`crate::vr::swapchain`]) and the native-resolution driver
//! ([`crate::vr::resolution`]) so the engine renders each eye at exactly the swapchain size.

use crate::{config::Config, vr::state::VR_STATE};

/// The per-eye render resolution the engine should target while a session is running: the runtime's
/// recommended view size × [`VrConfig::resolution_scale`](crate::vr::VrConfig::resolution_scale),
/// matching the stereo swapchain so the blit is a straight scale-1 pass. `None` when no session is up
/// or the recommended size is unknown. Read by [`crate::vr::resolution`] once per frame.
pub fn native_eye_resolution() -> Option<(u32, u32)> {
    let cfg = Config::lock_query(|c| c.vr.clone());
    let state = VR_STATE.lock();
    if !state.is_running() {
        return None;
    }
    state.eye_resolution(&cfg)
}

/// The resolution the engine's scene render targets should be built at while a session runs. Normally
/// the per-eye [`native_eye_resolution`]; under single-pass double-wide it is **2x that width**, so a
/// single walk renders both eye-halves side by side into one target. The XR swapchain and per-eye
/// capture textures stay per-eye width, so the collapse's capture split copies each full-width half
/// straight into its eye texture. `None` when no session is up. Read by [`crate::vr::resolution`] once
/// per frame.
pub fn engine_render_resolution() -> Option<(u32, u32)> {
    let (width, height) = native_eye_resolution()?;
    if crate::stereo::single_pass::double_wide_active() {
        Some((width.saturating_mul(2), height))
    } else {
        Some((width, height))
    }
}

/// Scale a raw recommended per-eye view size by `resolution_scale`, clamped to a small positive
/// minimum (and at least 1 px each axis), and round the width up to [`EYE_WIDTH_ALIGNMENT`].
///
/// Shared by the swapchain and the native-resolution driver so the engine renders each eye at exactly
/// the swapchain size -- which is why the rounding belongs here and is unconditional rather than
/// applied only under the collapse. Rounding one of the two would make them disagree, and the collapse
/// can be toggled at runtime while the swapchain keeps the size it was created at.
///
/// The cost is at most 63 extra columns per eye (~1.6% more pixels at the width above), paid whether
/// or not the collapse is on. That is cheap against a lighting fix that otherwise cannot run at all.
pub(super) fn scaled_eye_size(width: u32, height: u32, resolution_scale: f32) -> (u32, u32) {
    let scale = resolution_scale.max(0.1);
    let w = ((width as f32) * scale).round() as u32;
    let h = ((height as f32) * scale).round() as u32;
    (align_up(w.max(1), EYE_WIDTH_ALIGNMENT), h.max(1))
}

/// The alignment the per-eye render width is rounded up to.
///
/// The clustered lighting grid is a 64-pixel tile lattice. Under the single-pass collapse the eye seam
/// falls at the middle of a double-wide target, so it lands on a whole tile column only when each
/// eye's width is a whole number of tiles -- otherwise a partial tile column straddles the seam, there
/// is no correct way to split it between the eyes, and the per-eye grid build declines. A runtime's
/// recommended width is arbitrary (2015 px on the headset this was found with, giving a 4030 px target
/// that is not a multiple of 128), so left alone the split effectively never engages.
const EYE_WIDTH_ALIGNMENT: u32 = 64;

/// Round `value` up to the next multiple of `alignment`, saturating rather than wrapping.
fn align_up(value: u32, alignment: u32) -> u32 {
    match value % alignment {
        0 => value,
        remainder => value.saturating_add(alignment - remainder),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the alignment: whatever the runtime recommends, the doubled width the
    /// collapse renders into must land on a whole froxel tile column, i.e. be a multiple of twice the
    /// 64-pixel tile size. 2015 is the width that exposed this -- it produced a 4030 px target and the
    /// per-eye light grid declined on every frame of every session.
    #[test]
    fn double_wide_width_lands_on_a_whole_tile_column() {
        for recommended in [1, 2015, 2016, 2048, 1080, 1440, 4095] {
            let (width, _) = scaled_eye_size(recommended, 1000, 1.0);
            assert_eq!(
                (width * 2) % (2 * EYE_WIDTH_ALIGNMENT),
                0,
                "per-eye width {width} from recommended {recommended} does not double to a whole \
                 tile column",
            );
            assert!(width >= recommended, "alignment must not shrink the render");
        }
    }

    /// Rounding never reduces the size and never wraps at the top of the range.
    #[test]
    fn align_up_rounds_upward_and_saturates() {
        assert_eq!(align_up(1, 64), 64);
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(65, 64), 128);
        assert_eq!(align_up(u32::MAX, 64), u32::MAX);
    }

    /// A resolution scale still applies; the alignment is on top of it, not instead of it.
    #[test]
    fn resolution_scale_still_applies_under_alignment() {
        let (full, _) = scaled_eye_size(2048, 1000, 1.0);
        let (half, _) = scaled_eye_size(2048, 1000, 0.5);
        assert!(half < full, "a 0.5 scale must still render smaller");
        assert_eq!(half % EYE_WIDTH_ALIGNMENT, 0);
    }
}
