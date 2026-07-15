//! The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35).
//!
//! `CRenderBlockDeferredLighting::DrawClustered` reconstructs a symmetric frustum from the vertical
//! FOV and aspect ratio, then uploads 8 floats (2 vec4s) to fragment constant buffer 1 (cb1) as
//! per-tile frustum edge bounds. The formula `boundX(i) = horiz * (1 - 2*i/tileCountX)` is always
//! centered on the optical axis and cannot encode the off-axis shift that VR per-eye projections
//! introduce — so lights are assigned to the wrong 64-pixel tiles, producing blocky, screen-aligned
//! lighting artifacts in VR.
//!
//! The geometry proxy transform (cb0, uploaded earlier in `DrawClustered`) is built from
//! `RenderContext::m_ProjectionF`, which already carries the off-axis projection (written by the
//! camera hook before `SetupRenderCamera`). So cb0 is correct; only cb1 needs overriding.
//!
//! Because the cb1 upload and the light-proxy draws both happen inside `DrawClustered`, we cannot
//! re-upload after the original returns. Instead, a thread-local flag is set around the original
//! `DrawClustered` call, and a detour on `Graphics::SetFragmentProgramConstants` intercepts the cb1
//! upload (identified by `cb_index=1, start_offset=0, count=2`) and replaces the data with
//! off-axis-derived values computed from the per-eye projection matrix. The second
//! `SetFragmentProgramConstants` call in `DrawClustered` (tile grid dimensions, `count=1`) is not
//! intercepted.
//!
//! Both `DrawClustered` and its `SetFragmentProgramConstants` calls run on the render thread, so the
//! thread-local flag correctly scopes the interception.

use std::{cell::Cell, ffi::c_void};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{HContext_t, HTexture_t, RenderContext},
    render_block::RenderBlockDeferredLighting,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&DRAW_CLUSTERED_BINDER)
        .with_static_binder(&SET_FRAGMENT_PROGRAM_CONSTANTS_BINDER)
}

thread_local! {
    /// Set while the original `DrawClustered` is running, so the `SetFragmentProgramConstants`
    /// detour knows to intercept the cb1 tile-bounds upload.
    static CLUSTERED_ACTIVE: Cell<bool> = const { Cell::new(false) };

    /// The pre-computed off-axis cb1 values for the current `DrawClustered` call, or `None` when
    /// no VR frame is in flight (flatscreen, or the fix is disabled).
    static OFF_AXIS_CB1: Cell<Option<[f32; 8]>> = const { Cell::new(None) };
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockDeferredLighting::DrawClustered_ADDRESS
)]
fn draw_clustered(
    this: *const RenderBlockDeferredLighting,
    rc: *mut RenderContext,
    a3: *mut c_void,
    a4: *mut HTexture_t,
) {
    // When the fix is disabled, call through without setting the thread-local flag.
    if !Config::lock_query(|c| c.stereo.fix_clustered_light_frustum) {
        DRAW_CLUSTERED.get().unwrap().call(this, rc, a3, a4);
        return;
    }

    // Pre-compute the off-axis cb1 values for this eye, if a VR frame is in flight.
    let off_axis_cb1 = crate::vr::render_params(crate::stereo::draw_index()).and_then(|params| {
        // SAFETY: `rc` is the live render context for this dispatch; the caller (the engine's
        // draw dispatch) guarantees it is valid for the duration of `DrawClustered`.
        let rc_ref = unsafe { rc.as_ref() }?;
        Some(compute_off_axis_tile_bounds(
            &params.projection_standard,
            rc_ref,
        ))
    });

    if let Some(ref cb1) = off_axis_cb1 {
        CLUSTERED_ACTIVE.with(|f| f.set(true));
        OFF_AXIS_CB1.with(|slot| slot.set(Some(*cb1)));
    }

    DRAW_CLUSTERED.get().unwrap().call(this, rc, a3, a4);

    CLUSTERED_ACTIVE.with(|f| f.set(false));
    OFF_AXIS_CB1.with(|slot| slot.set(None));
}

#[detour(address = jc3gi::graphics_engine::draw::SetFragmentProgramConstants_ADDRESS)]
fn set_fragment_program_constants(
    ctx: *mut HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    // Intercept the cb1 tile-bounds upload during DrawClustered's light-assignment pass.
    // The upload is: cb_index=1, start_offset=0, count=2 (8 floats = 2 vec4s).
    // The second SetFragmentProgramConstants call in DrawClustered (tile grid dimensions)
    // has count=1, so it is not intercepted.
    if CLUSTERED_ACTIVE.with(|f| f.get())
        && cb_index == 1
        && start_offset == 0
        && count == 2
        && let Some(cb1) = OFF_AXIS_CB1.with(|slot| slot.get())
    {
        SET_FRAGMENT_PROGRAM_CONSTANTS.get().unwrap().call(
            ctx,
            cb_index,
            start_offset,
            cb1.as_ptr(),
            count,
        );
        return;
    }
    SET_FRAGMENT_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(ctx, cb_index, start_offset, data, count);
}

/// Compute the 8-float cb1 tile-bounds array from the off-axis projection matrix.
///
/// The symmetric formula in the original `DrawClustered` is:
///   horiz = tan(FOV/2) * aspect
///   vert = tan(FOV/2)
///   cb1[0] = -2 * horiz / tileCountX   (horizontal slope)
///   cb1[1] = horiz * (1 + 1/tileCountX) (horizontal max)
///   cb1[2] = horiz * (1 - 1/tileCountX) (horizontal min)
///   cb1[3] = 0
///   cb1[4..7] = same for vertical
///
/// For the off-axis case, replace `horiz` with the actual right bound and `2*horiz` (the full
/// extent) with `(right - left)`:
///   cb1[0] = -(right - left) / tileCountX
///   cb1[1] = right + (right - left) / (2 * tileCountX)
///   cb1[2] = right - (right - left) / (2 * tileCountX)
///   cb1[3] = 0
///   cb1[4] = -(top - bottom) / tileCountY
///   cb1[5] = top + (top - bottom) / (2 * tileCountY)
///   cb1[6] = top - (top - bottom) / (2 * tileCountY)
///   cb1[7] = 0
///
/// In the symmetric case, right = horiz and left = -horiz, so (right - left) = 2*horiz. The
/// off-axis formula generalizes this to arbitrary left/right bounds.
///
/// The frustum bounds are extracted from the projection matrix (row-major, row-vector):
///   right  = (1 + m[8]) / m[0]
///   left   = (m[8] - 1) / m[0]
///   top    = (1 + m[9]) / m[5]
///   bottom = (m[9] - 1) / m[5]
///
/// The reverse-Z remap (applied by `SetupRenderCamera` to `m_ProjectionF`) only changes column 2
/// (indices 2, 6, 10, 14), so m[0], m[5], m[8], m[9] are unaffected and the bounds can be extracted
/// from either the standard-depth or reverse-Z'd matrix.
fn compute_off_axis_tile_bounds(projection: &[f32; 16], rc: &RenderContext) -> [f32; 8] {
    let tile_count_x = rc.m_DisplayWidth as f32 * 0.015625; // / 64
    let tile_count_y = rc.m_DisplayHeight as f32 * 0.015625;
    tile_bounds_from_projection(projection, tile_count_x, tile_count_y)
}

/// The pure-math core of [`compute_off_axis_tile_bounds`], factored out for unit testing without a
/// live `RenderContext`.
fn tile_bounds_from_projection(
    projection: &[f32; 16],
    tile_count_x: f32,
    tile_count_y: f32,
) -> [f32; 8] {
    let right = (1.0 + projection[8]) / projection[0];
    let left = (projection[8] - 1.0) / projection[0];
    let top = (1.0 + projection[9]) / projection[5];
    let bottom = (projection[9] - 1.0) / projection[5];

    let h_extent = right - left;
    let v_extent = top - bottom;

    let h_half_tile = h_extent / (2.0 * tile_count_x);
    let v_half_tile = v_extent / (2.0 * tile_count_y);

    [
        -h_extent / tile_count_x, // horizontal slope
        right + h_half_tile,      // horizontal max
        right - h_half_tile,      // horizontal min
        0.0,
        -v_extent / tile_count_y, // vertical slope
        top + v_half_tile,        // vertical max
        top - v_half_tile,        // vertical min
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::projection::{Fov, OffAxisProjection};

    /// Extract the frustum bounds (right, left, top, bottom) from a row-major projection matrix.
    fn frustum_bounds(projection: &[f32; 16]) -> (f32, f32, f32, f32) {
        let right = (1.0 + projection[8]) / projection[0];
        let left = (projection[8] - 1.0) / projection[0];
        let top = (1.0 + projection[9]) / projection[5];
        let bottom = (projection[9] - 1.0) / projection[5];
        (right, left, top, bottom)
    }

    /// Tile counts for a 1920×1080 display (the engine divides each dimension by 64).
    const TILE_COUNT_X: f32 = 1920.0 * 0.015625;
    const TILE_COUNT_Y: f32 = 1080.0 * 0.015625;

    /// The off-axis tile-bounds computation must produce the same values as the original symmetric
    /// formula when given a symmetric projection matrix (left = -right, bottom = -top).
    #[test]
    fn test_off_axis_matches_symmetric_for_centered_frustum() {
        let half_fov_y = 50.0_f32.to_radians();
        let half_fov_x = 45.0_f32.to_radians();
        let fov = Fov {
            left: -half_fov_x,
            right: half_fov_x,
            up: half_fov_y,
            down: -half_fov_y,
        };
        let proj = OffAxisProjection::new(fov, 0.1, 38400.0).standard_depth;

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y);

        // The original symmetric formula from the decompile:
        //   v21 = tan(FOV/2)           // half vertical FOV
        //   v22 = v21 * aspect          // horiz = tan(FOV/2) * aspect
        //   v14 = 1 / tileCountX
        //   v15 = 1 / tileCountY
        //   cb1[0] = v14 * -2 * v22    = -2 * horiz / tileCountX
        //   cb1[1] = (v14 + 1) * v22   = (1/tileCountX + 1) * horiz
        //   cb1[2] = (1 - v14) * v22   = (1 - 1/tileCountX) * horiz
        //   cb1[3] = 0
        //   cb1[4] = v15 * -2 * v21    = -2 * vert / tileCountY
        //   cb1[5] = (v15 + 1) * v21   = (1/tileCountY + 1) * vert
        //   cb1[6] = (1 - v15) * v21   = (1 - 1/tileCountY) * vert
        //   cb1[7] = 0
        let vert = half_fov_y.tan();
        let aspect = half_fov_x.tan() / half_fov_y.tan();
        let horiz = vert * aspect;
        let inv_tx = 1.0 / TILE_COUNT_X;
        let inv_ty = 1.0 / TILE_COUNT_Y;

        let expected = [
            -2.0 * horiz * inv_tx,
            (inv_tx + 1.0) * horiz,
            (1.0 - inv_tx) * horiz,
            0.0,
            -2.0 * vert * inv_ty,
            (inv_ty + 1.0) * vert,
            (1.0 - inv_ty) * vert,
            0.0,
        ];

        for i in 0..8 {
            assert!(
                (cb1[i] - expected[i]).abs() < 1e-4,
                "cb1[{i}]: off-axis {} vs symmetric {}",
                cb1[i],
                expected[i]
            );
        }
    }

    /// The off-axis tile-bounds computation must produce asymmetric bounds (non-zero center shift)
    /// when given an asymmetric projection matrix.
    #[test]
    fn test_off_axis_produces_asymmetric_bounds() {
        let fov = Fov {
            left: -30.0_f32.to_radians(),
            right: 50.0_f32.to_radians(),
            up: 35.0_f32.to_radians(),
            down: -45.0_f32.to_radians(),
        };
        let proj = OffAxisProjection::new(fov, 0.1, 38400.0).standard_depth;

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y);

        // cb1[1] = right + half_tile, cb1[2] = right - half_tile
        // So right = (cb1[1] + cb1[2]) / 2, and the center = right - extent/2 = (right + left) / 2.
        // The center is non-zero iff the frustum is asymmetric.
        let h_center = (cb1[1] + cb1[2]) / 2.0;
        let v_center = (cb1[5] + cb1[6]) / 2.0;

        assert!(
            h_center.abs() > 0.01,
            "horizontal center shift is {h_center}, expected non-zero"
        );
        assert!(
            v_center.abs() > 0.01,
            "vertical center shift is {v_center}, expected non-zero"
        );

        // Verify the center matches the projection's frustum center.
        let (right, left, top, bottom) = frustum_bounds(&proj);
        let expected_h_center = (right + left) / 2.0;
        let expected_v_center = (top + bottom) / 2.0;
        assert!(
            (h_center - expected_h_center).abs() < 1e-4,
            "horizontal center {h_center} vs expected {expected_h_center}"
        );
        assert!(
            (v_center - expected_v_center).abs() < 1e-4,
            "vertical center {v_center} vs expected {expected_v_center}"
        );
    }

    /// The frustum-bound extraction must match the known tangent values for a given FOV.
    #[test]
    fn test_frustum_bounds_from_projection() {
        let fov = Fov {
            left: -40.0_f32.to_radians(),
            right: 40.0_f32.to_radians(),
            up: 40.0_f32.to_radians(),
            down: -40.0_f32.to_radians(),
        };
        let proj = OffAxisProjection::new(fov, 0.1, 38400.0).standard_depth;

        let (right, left, top, bottom) = frustum_bounds(&proj);

        // For a symmetric frustum, right = tan(angleRight), left = tan(angleLeft), etc.
        assert!((right - fov.right.tan()).abs() < 1e-5, "right: {right}");
        assert!((left - fov.left.tan()).abs() < 1e-5, "left: {left}");
        assert!((top - fov.up.tan()).abs() < 1e-5, "top: {top}");
        assert!((bottom - fov.down.tan()).abs() < 1e-5, "bottom: {bottom}");
    }
}
