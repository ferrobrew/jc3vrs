//! The off-axis per-tile frustum bounds the mod uploads in place of the engine's symmetric ones,
//! over the whole grid or over one eye's half of it.

use jc3gi::types::math::Matrix4;

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
///
/// `tile_count_x` is the count the horizontal bounds are quantised over -- the whole grid's, or one
/// eye's half of it under the per-eye split -- and `eye_column_offset` is how many multiples of that
/// count the pixel shader's absolute tile index runs ahead of the local one. The pixel shader derives
/// its frustum from `SV_Position`, which includes the viewport's `TopLeftX`, so shifting the local
/// index `j` to the absolute `i = j + eye * tile_count_x` is what makes the same 8 floats describe a
/// half of the grid that does not start at column 0. `eye_column_offset == 0` reduces to the
/// whole-grid form.
pub(super) fn tile_bounds_from_projection(
    projection: &Matrix4,
    tile_count_x: f32,
    tile_count_y: f32,
    eye_column_offset: usize,
) -> [f32; 8] {
    let d = &projection.data;
    let right = (1.0 + d[8]) / d[0];
    let left = (d[8] - 1.0) / d[0];
    let top = (1.0 + d[9]) / d[5];
    let bottom = (d[9] - 1.0) / d[5];

    let h_extent = right - left;
    let v_extent = top - bottom;

    let h_half_tile = h_extent / (2.0 * tile_count_x);
    let v_half_tile = v_extent / (2.0 * tile_count_y);
    // Substituting `i -> i - eye * tile_count_x` into `bound(i) = right - h_extent * i / tile_count_x`
    // leaves the slope alone and shifts both biases by `eye * h_extent`.
    let h_origin = right + eye_column_offset as f32 * h_extent;

    [
        -h_extent / tile_count_x, // horizontal slope
        h_origin + h_half_tile,   // horizontal max
        h_origin - h_half_tile,   // horizontal min
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
    use crate::{
        hooks::graphics_engine::clustered_lighting::split::TILE_SIZE,
        vr::projection::{Fov, OffAxisProjection},
    };

    /// Extract the frustum bounds (right, left, top, bottom) from a row-major projection matrix.
    fn frustum_bounds(projection: &Matrix4) -> (f32, f32, f32, f32) {
        let d = &projection.data;
        let right = (1.0 + d[8]) / d[0];
        let left = (d[8] - 1.0) / d[0];
        let top = (1.0 + d[9]) / d[5];
        let bottom = (d[9] - 1.0) / d[5];
        (right, left, top, bottom)
    }

    /// Tile counts for a 1920×1080 display (the engine divides each dimension by 64).
    const TILE_COUNT_X: f32 = 1920.0 / TILE_SIZE as f32;
    const TILE_COUNT_Y: f32 = 1080.0 / TILE_SIZE as f32;

    fn asymmetric_fov() -> Fov {
        Fov {
            left: -30.0_f32.to_radians(),
            right: 50.0_f32.to_radians(),
            up: 35.0_f32.to_radians(),
            down: -45.0_f32.to_radians(),
        }
    }

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

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y, 0);

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
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y, 0);

        // Recover the frustum centre from the uploaded constants, which is what makes this a
        // round-trip check rather than a restatement of the formula. `cb1[1]`/`cb1[2]` are
        // `right +/- half_tile`, so their mean is `right` -- *not* the centre. The extent comes from
        // the slope (`cb1[0] = -extent / tile_count`), and the centre is then `right - extent/2`,
        // which equals `(right + left) / 2`. It is non-zero iff the frustum is asymmetric.
        let h_extent = -cb1[0] * TILE_COUNT_X;
        let v_extent = -cb1[4] * TILE_COUNT_Y;
        let h_center = (cb1[1] + cb1[2]) / 2.0 - h_extent / 2.0;
        let v_center = (cb1[5] + cb1[6]) / 2.0 - v_extent / 2.0;

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

    /// The per-eye bounds evaluated at the pixel shader's own sample point -- the **absolute** tile
    /// index plus a half -- must reproduce that eye's frustum edges over that eye's half of the grid.
    /// This is the property the whole split rests on: the same 8 floats have to describe a run of
    /// tiles that does not start at column 0.
    #[test]
    fn per_eye_tile_bounds_are_affine_in_the_absolute_tile_index() {
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;
        let (right, left, top, bottom) = frustum_bounds(&proj);
        let half_tiles = TILE_COUNT_X / 2.0;

        for eye in 0..2 {
            let cb1 = tile_bounds_from_projection(&proj, half_tiles, TILE_COUNT_Y, eye);
            for j in 0..half_tiles as usize {
                // The pixel shader evaluates `v0.x * slope + bias` at the absolute tile index + 0.5.
                let absolute = eye as f32 * half_tiles + j as f32 + 0.5;
                let (max, min) = (absolute * cb1[0] + cb1[1], absolute * cb1[0] + cb1[2]);
                // Tile `j` of this eye spans `[right - extent*(j+1)/T, right - extent*j/T]`.
                let extent = right - left;
                let expected_max = right - extent * j as f32 / half_tiles;
                let expected_min = right - extent * (j + 1) as f32 / half_tiles;
                assert!(
                    (max - expected_max).abs() < 1e-4 && (min - expected_min).abs() < 1e-4,
                    "eye {eye} tile {j}: [{min}, {max}] vs expected [{expected_min}, {expected_max}]",
                );
            }
            // The vertical row does not halve: it must still span the full frustum height.
            let v_extent = -cb1[4] * TILE_COUNT_Y;
            assert!(
                (v_extent - (top - bottom)).abs() < 1e-4,
                "eye {eye}: vertical extent {v_extent} vs expected {}",
                top - bottom,
            );
            assert!(
                (cb1[5] - (top + v_extent / (2.0 * TILE_COUNT_Y))).abs() < 1e-4,
                "eye {eye}: vertical max {} is not the top edge plus a half tile",
                cb1[5],
            );
        }
    }

    /// The two eyes' halves must tile the whole grid exactly once: eye 0's first column starts at its
    /// own right edge, eye 1's last column ends at its own left edge, and neither reaches into the
    /// other's absolute tile range.
    #[test]
    fn per_eye_tile_bounds_cover_each_half_exactly_once() {
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;
        let (right, left, _, _) = frustum_bounds(&proj);
        let half_tiles = TILE_COUNT_X / 2.0;

        for eye in 0..2 {
            let cb1 = tile_bounds_from_projection(&proj, half_tiles, TILE_COUNT_Y, eye);
            let first = eye as f32 * half_tiles + 0.5;
            let last = eye as f32 * half_tiles + half_tiles - 0.5;
            assert!(
                (first * cb1[0] + cb1[1] - right).abs() < 1e-4,
                "eye {eye}: first tile's max bound is not the frustum's right edge",
            );
            assert!(
                (last * cb1[0] + cb1[2] - left).abs() < 1e-4,
                "eye {eye}: last tile's min bound is not the frustum's left edge",
            );
        }
    }
}
