//! Off-axis (asymmetric-frustum) projection matrices in the JC3 engine's conventions.
//!
//! Real HMDs have asymmetric, per-eye frusta -- the pupil is not centred on its display half -- so
//! each eye needs an off-centre perspective built from the four `XrFovf` half-angles (`angleLeft`,
//! `angleRight`, `angleUp`, `angleDown`). This module turns those angles into a matrix laid out the
//! way the engine's `Camera::m_Projection` is: **row-major, row-vector** (`clip = p · M`), the D3D
//! convention documented in `docs/engine/rendering.md` §2.6.
//!
//! The math is built with `glam`, which is column-major, column-vector (`clip = M * p`). A row-major
//! row-vector matrix's flat 16-float array is bit-identical to `to_cols_array()` of the
//! mathematically-equivalent glam column-vector matrix -- engine row `i` is glam column `i` -- so the
//! [`jc3gi::types::math::Matrix4`] this module returns (whose `From<glam::Mat4>` impl calls
//! `to_cols_array()`) carries exactly the engine's layout, with no transpose needed.
//!
//! The `standard_depth` matrix is built element-for-element the way the engine's
//! `CMatrix4f::PerspectiveOffCenter` builds `Camera::m_Projection` (verified against the release
//! build, `docs/engine/rendering.md` §2.9): the engine passes near-plane extents (`near·tan θ`),
//! this module passes the tangents directly, and `near` cancels out of every term except the depth
//! column, so the two matrices are identical. The engine's finite far plane defaults to `38400` and
//! near to `0.1` (the `Camera` constructor values, §2.9); the mod feeds the same near/far so the
//! frustum matches and the horizon does not clip.
//!
//! Two depth conventions are produced (see `docs/engine/rendering.md` §2.7, §2.9):
//!
//! - [`OffAxisProjection::standard_depth`]: a standard (non-reversed) projection, NDC z in `[0, 1]`
//!   with near → 0 and far → 1. **This is the one to write into `m_Projection` before
//!   `SetupRenderCamera`** (blocker 1): the engine then applies its own reverse-Z remap (`z' = w - z`)
//!   and TAA jitter to it exactly once, matching every other camera. `SetupRenderCamera` consumes the
//!   pre-written `m_Projection` in place (it does not rebuild it from FOV/near/far, §2.9), so this
//!   write reaches the GPU. Feeding an already-reversed matrix into that window double-applies the
//!   remap -- the §2.7 wedge bug.
//! - [`OffAxisProjection::reverse_z`]: the same projection with the engine's reverse-Z remap already
//!   applied (near → 1, far → 0). This is for the §2.7 *alternative* path -- writing the projection
//!   *after* `SetupRenderCamera` has run (when bit `0x20` is set, so the engine will not re-reverse
//!   it) -- and must be paired with a manual jitter / VP rebuild. Not the preferred path; provided so
//!   both conventions are available and explicitly labelled.
//!
//! The math is deliberately free of any OpenXR type so it is unit-testable on the Linux host: the
//! caller in [`crate::vr`] converts `xr::Fovf` into [`Fov`].

use glam::{Mat4, Vec4};
use jc3gi::types::math::Matrix4;

/// A symmetric-or-asymmetric field of view, as four half-angles in radians measured from the view
/// axis. Matches the sign convention of `XrFovf`: `left` and `down` are negative, `right` and `up`
/// are positive, for a forward-facing view.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Fov {
    pub left: f32,
    pub right: f32,
    pub up: f32,
    pub down: f32,
}

/// A per-eye off-axis projection in both of the engine's depth conventions. Both matrices are
/// row-major, row-vector (`clip = p · M`), the layout of `Camera::m_Projection`.
#[derive(Copy, Clone, Debug)]
pub struct OffAxisProjection {
    /// Standard depth (NDC z in `[0, 1]`, near → 0, far → 1). Write this into `m_Projection`
    /// *before* `SetupRenderCamera` so the engine reverse-Z's and jitters it once (the preferred
    /// path, `docs/engine/rendering.md` §2.7).
    pub standard_depth: Matrix4,
    /// Reverse-Z depth (near → 1, far → 0), the engine's `z' = w - z` remap already applied. Write
    /// this *after* `SetupRenderCamera` (the §2.7 alternative path); you then own jitter and the
    /// VP rebuild.
    pub reverse_z: Matrix4,
}

impl OffAxisProjection {
    /// Build the off-axis projection for `fov` with the given clip planes. `near` and `far` are the
    /// positive view-space distances to the near and far planes (`far > near > 0`); reverse-Z
    /// tolerates a very distant far plane.
    pub fn new(fov: Fov, near: f32, far: f32) -> Self {
        let standard_depth = perspective_off_center_rh(fov, near, far);
        Self {
            standard_depth: Matrix4::from(standard_depth),
            reverse_z: Matrix4::from(apply_reverse_z(standard_depth)),
        }
    }
}

/// A standard right-handed off-centre perspective in the engine's row-major, row-vector layout (as a
/// glam column-vector matrix -- see the module docs for the equivalence), with NDC z in `[0, 1]`
/// (`DirectXMath`'s `PerspectiveOffCenterRH` convention). The horizontal/vertical extents come from
/// the FOV tangents at unit depth, so `near` cancels out of every term except the depth column.
fn perspective_off_center_rh(fov: Fov, near: f32, far: f32) -> Mat4 {
    let tl = fov.left.tan();
    let tr = fov.right.tan();
    let tu = fov.up.tan();
    let td = fov.down.tan();

    let inv_w = 1.0 / (tr - tl);
    let inv_h = 1.0 / (tu - td);
    let depth = far / (near - far);

    // Engine row `i` is glam column `i` (see the module docs), so each `Vec4` below is one engine row.
    Mat4::from_cols(
        Vec4::new(2.0 * inv_w, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 2.0 * inv_h, 0.0, 0.0),
        Vec4::new((tl + tr) * inv_w, (td + tu) * inv_h, depth, -1.0),
        Vec4::new(0.0, 0.0, near * depth, 0.0),
    )
}

/// Apply the engine's reverse-Z remap (`z' = w - z`) to a standard-depth projection: for each row,
/// column 2 becomes column 3 minus column 2. This is exactly what `SetupRenderCamera` /
/// `RecalcProjection` do to `m_Projection` on a render camera (`docs/engine/rendering.md` §2.3, §2.7).
///
/// In glam terms (engine row `i` == glam column `i`, engine column `j` == glam component `j`), the
/// remap becomes: each column's `.z` component becomes that column's `.w` minus its `.z`.
fn apply_reverse_z(m: Mat4) -> Mat4 {
    Mat4::from_cols(
        remap_col_z(m.x_axis),
        remap_col_z(m.y_axis),
        remap_col_z(m.z_axis),
        remap_col_z(m.w_axis),
    )
}

/// `col.z = col.w - col.z`, the per-column half of [`apply_reverse_z`].
fn remap_col_z(col: Vec4) -> Vec4 {
    Vec4::new(col.x, col.y, col.w - col.z, col.w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4 as GlamMat4;

    /// `m[row * 4 + col]` for the row-major matrices this module produces, reading through the
    /// engine-row == glam-column equivalence.
    fn at(m: &Matrix4, row: usize, col: usize) -> f32 {
        m.data[row * 4 + col]
    }

    /// Project a view-space point through a row-major, row-vector projection (`clip = p · M`) and
    /// return the resulting NDC point (perspective divide applied). Implemented as glam
    /// `mat_glam * vec4`, which is equivalent because `mat_glam` (built via `from_cols_array` on the
    /// engine's flat array) is the transpose-equivalent column-vector matrix: `M_glam * p == p · M`
    /// when `M_glam`'s columns are `M`'s rows, which is exactly what `from_cols_array` on the row-major
    /// flat array gives.
    fn project(m: &Matrix4, p: [f32; 4]) -> [f32; 3] {
        let mat_glam = GlamMat4::from(*m);
        let clip = mat_glam * Vec4::from(p);
        [clip.x / clip.w, clip.y / clip.w, clip.z / clip.w]
    }

    /// A symmetric FOV must reproduce the reference D3D perspective. glam's `perspective_rh` is a
    /// column-major, column-vector matrix with NDC z in `[0, 1]`; its transpose is our row-vector
    /// matrix, and a transpose stored row-major has the identical flat array as the original stored
    /// column-major -- so our `standard_depth` array equals glam's `to_cols_array()` elementwise.
    #[test]
    fn symmetric_matches_reference_perspective() {
        let half_x = 45.0_f32.to_radians();
        let half_y = 40.0_f32.to_radians();
        let near = 0.1;
        let far = 100.0;
        let fov = Fov {
            left: -half_x,
            right: half_x,
            up: half_y,
            down: -half_y,
        };

        let proj = OffAxisProjection::new(fov, near, far).standard_depth;

        // fov_y is the full vertical angle; aspect = tan(half_x) / tan(half_y).
        let fov_y = 2.0 * half_y;
        let aspect = half_x.tan() / half_y.tan();
        let reference = GlamMat4::perspective_rh(fov_y, aspect, near, far).to_cols_array();

        for (a, b) in proj.data.iter().zip(reference.iter()) {
            assert!((a - b).abs() < 1e-5, "got {a}, expected {b}");
        }

        // A symmetric frustum has no off-centre shift.
        assert!(at(&proj, 2, 0).abs() < 1e-6);
        assert!(at(&proj, 2, 1).abs() < 1e-6);
    }

    /// Asymmetric angles must place the projection centre off-axis with the correct signs: the
    /// direction through the middle of the frustum must project to NDC `(0, 0)`, and the off-centre
    /// terms must carry the sign of the frustum's asymmetry.
    #[test]
    fn asymmetric_off_center_signs() {
        // Wider to the right and downward (right pupil looking through an off-centre panel).
        let fov = Fov {
            left: -30.0_f32.to_radians(),
            right: 50.0_f32.to_radians(),
            up: 35.0_f32.to_radians(),
            down: -45.0_f32.to_radians(),
        };
        let proj = OffAxisProjection::new(fov, 0.1, 100.0).standard_depth;

        // The frustum is wider on the right (|right| > |left|) => positive horizontal shift term;
        // wider below (|down| > |up|) => negative vertical shift term.
        assert!(at(&proj, 2, 0) > 0.0, "horizontal off-centre term sign");
        assert!(at(&proj, 2, 1) < 0.0, "vertical off-centre term sign");

        // The ray through the middle of the frustum (at unit depth, view looks down -z) projects to
        // the NDC origin.
        let cx = 0.5 * (fov.left.tan() + fov.right.tan());
        let cy = 0.5 * (fov.up.tan() + fov.down.tan());
        let ndc = project(&proj, [cx, cy, -1.0, 1.0]);
        assert!(ndc[0].abs() < 1e-5, "centre x = {}", ndc[0]);
        assert!(ndc[1].abs() < 1e-5, "centre y = {}", ndc[1]);
    }

    /// The `standard_depth` matrix must match the engine's `CMatrix4f::PerspectiveOffCenter`
    /// element-for-element (`docs/engine/rendering.md` §2.9). The engine builds it from the
    /// near-plane extents `x = near·tan θ`, `y = near·tan φ`; this reproduces those exact formulas
    /// with the game's real default near/far and asserts equality against the mod's builder.
    #[test]
    fn standard_depth_matches_engine_perspective_off_center() {
        let fov = Fov {
            left: -35.0_f32.to_radians(),
            right: 42.0_f32.to_radians(),
            up: 40.0_f32.to_radians(),
            down: -38.0_f32.to_radians(),
        };
        // The engine's Camera constructor defaults: m_Near = 0.1, m_Far = 38400 (0x47160000).
        let near = 0.1_f32;
        let far = 38400.0_f32;

        // Near-plane extents, exactly what RecalcProjection passes to PerspectiveOffCenter.
        let x_min = near * fov.left.tan();
        let x_max = near * fov.right.tan();
        let y_min = near * fov.down.tan();
        let y_max = near * fov.up.tan();

        // CMatrix4f::PerspectiveOffCenter, flat row-major `e[]` (the engine's field layout).
        let mut engine = [0.0f32; 16];
        engine[0] = (2.0 * near) / (x_max - x_min);
        engine[5] = (2.0 * near) / (y_max - y_min);
        engine[8] = (x_min + x_max) / (x_max - x_min);
        engine[9] = (y_min + y_max) / (y_max - y_min);
        engine[10] = far / (near - far);
        engine[11] = -1.0;
        engine[14] = (near * far) / (near - far);

        let proj = OffAxisProjection::new(fov, near, far).standard_depth;
        for (i, (a, b)) in proj.data.iter().zip(engine.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "element {i}: mod {a}, engine {b}");
        }
    }

    /// The engine's reverse-Z remap (`SetupRenderCamera` / `RecalcProjection`, §2.9) is
    /// `col2 = col3 - col2` on the row-major matrix. Applying it to the engine's own standard
    /// projection must produce the mod's `reverse_z` matrix element-for-element, and map far → 0 at
    /// the game's real 38400 far plane.
    #[test]
    fn reverse_z_matches_engine_remap_at_real_far() {
        let fov = Fov {
            left: -40.0_f32.to_radians(),
            right: 40.0_f32.to_radians(),
            up: 40.0_f32.to_radians(),
            down: -40.0_f32.to_radians(),
        };
        let (near, far) = (0.1_f32, 38400.0_f32);
        let p = OffAxisProjection::new(fov, near, far);

        // Engine remap applied by hand to the standard matrix: e[row*4+2] = e[row*4+3] - e[row*4+2].
        let mut expected = p.standard_depth.data;
        for row in 0..4 {
            expected[row * 4 + 2] = expected[row * 4 + 3] - expected[row * 4 + 2];
        }
        for (i, (a, b)) in p.reverse_z.data.iter().zip(expected.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "element {i}: {a} vs {b}");
        }

        // Reverse-Z at the real far plane: far → 0, near → 1.
        let far_rev = project(&p.reverse_z, [0.0, 0.0, -far, 1.0])[2];
        let near_rev = project(&p.reverse_z, [0.0, 0.0, -near, 1.0])[2];
        assert!(far_rev.abs() < 1e-3, "rev far z = {far_rev}");
        assert!((near_rev - 1.0).abs() < 1e-3, "rev near z = {near_rev}");
    }

    /// Depth mapping: standard depth maps near → 0 and far → 1; the reverse-Z variant maps
    /// near → 1 and far → 0 (the engine convention after its `z' = w - z` remap, §2.7).
    #[test]
    fn depth_mapping_near_far() {
        let fov = Fov {
            left: -40.0_f32.to_radians(),
            right: 40.0_f32.to_radians(),
            up: 40.0_f32.to_radians(),
            down: -40.0_f32.to_radians(),
        };
        let near = 0.2;
        let far = 500.0;
        let p = OffAxisProjection::new(fov, near, far);

        // View looks down -z, so near/far planes are at z = -near / -far.
        let near_std = project(&p.standard_depth, [0.0, 0.0, -near, 1.0])[2];
        let far_std = project(&p.standard_depth, [0.0, 0.0, -far, 1.0])[2];
        assert!(near_std.abs() < 1e-4, "std near z = {near_std}");
        assert!((far_std - 1.0).abs() < 1e-4, "std far z = {far_std}");

        let near_rev = project(&p.reverse_z, [0.0, 0.0, -near, 1.0])[2];
        let far_rev = project(&p.reverse_z, [0.0, 0.0, -far, 1.0])[2];
        assert!((near_rev - 1.0).abs() < 1e-4, "rev near z = {near_rev}");
        assert!(far_rev.abs() < 1e-4, "rev far z = {far_rev}");
    }
}
