//! Character render-block detours: hide the local player's head from every non-shadow pass
//! (issue #19).
//!
//! A character model is one render block per material, and the same block objects are drawn for
//! every pass — shadow, depth prepass, GBuffer — branching internally on
//! [`RenderContext::m_RenderStatus`]. The hiding operates at the *bone* level, the true
//! granularity of skinning: Rico's skin material packs the face, arms, and hands into a single
//! block (and even a single 70-bone skin batch), so neither block-level nor batch-level skipping
//! can remove the head without taking the hands along. Instead, while one of the player's blocks
//! draws in a non-shadow pass, the palette upload (`SetMatrixPalette`) is fed a *copy* of the
//! bone matrices with the HEAD and facial bones collapsed — a zero rotation with the translation
//! set to the neck's published model-space position, in whichever matrix layout the block's
//! palette uses (detected per batch) — so every vertex weighted to them (face, eyes, ears, and
//! the hair riding the HEAD bone) contracts to a point inside the collar, while the hands keep
//! their own bones. Batches that are mostly facial are face gear and collapse wholesale. The
//! real palette is never touched, so the shadow passes draw a full, headful character that
//! follows the player-driven head pose.
//!
//! Ownership is settled by pointer identity: every draw receives its owning model instance's
//! embedded `CRBIInfo` as the `info` argument, and the character hook publishes the player's
//! instance-info pointers slot-for-slot — the hide matches the body-model slot only. Character
//! draws run concurrently on multiple render worker threads, so the spoof flag and the palette
//! copy are thread-local.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::{HContext_t, RenderContext},
        render_block::{
            Matrix3x4, RBIInfo, RenderBlockCharacter, RenderBlockCharacterSkin, SkinBatch,
        },
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&RENDER_BLOCK_CHARACTER_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_CHARACTER_DRAW_Z_BINDER)
        .with_static_binder(&RENDER_BLOCK_CHARACTER_SKIN_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_CHARACTER_SKIN_DRAW_Z_BINDER)
        .with_static_binder(&RENDER_BLOCK_CHARACTER_SET_MATRIX_PALETTE_BINDER)
        .with_static_binder(&RENDER_BLOCK_CHARACTER_SKIN_SET_MATRIX_PALETTE_BINDER)
}

/// One-shot diagnostics: while positive, each character-block draw near the player logs its
/// class, instance transform, and per-batch skeleton lookups, then decrements. Armed from the
/// debug UI to establish ground truth about the batch bone-index space and transform placement.
pub static DUMP_DRAWS: AtomicI32 = AtomicI32::new(0);

/// Publish the local player's root position pair (the T0 and T1 world-matrix translations), from
/// the character hook. Diagnostics only (the dump's distance columns and focus filter); ownership
/// uses the exact instance-info pointers below.
pub fn publish_player_root(t0: glam::Vec3, t1: glam::Vec3) {
    *PLAYER_ROOT.lock() = Some((t0, t1));
}

/// Publish the `CRBIInfo` pointers of the local player's model instances, slot-for-slot (zero for
/// empty slots), from the character hook. Every render-block draw receives its owning model
/// instance's embedded `CRBIInfo` as the `info` argument, so pointer identity is an *exact*
/// ownership test — unlike the earlier position-radius check, which failed at vehicle/wingsuit
/// speeds and false-matched nearby characters. The slots are kept distinct because they are
/// different models with different skeletons: the parachute occupies one of them, and collapsing
/// the body skeleton's bone indices on the chute's skeleton crumpled the canopy.
pub fn publish_player_rbi_infos(infos: &[usize; PLAYER_MODEL_SLOTS]) {
    for (slot, &info) in PLAYER_RBI_INFOS.iter().zip(infos) {
        slot.store(info, Ordering::Relaxed);
    }
}

/// The number of model-instance slots on `CAnimatedModel`.
pub const PLAYER_MODEL_SLOTS: usize = 8;

/// Publish the NECK bone's model-space position, from the character hook: the collapse target
/// point. Collapsed vertices contract here (inside the collar) rather than to each bone's own
/// position — vertices blended between a collapsed and a kept bone form a stretch cone to the
/// collapse point, and anchoring that point at the neck keeps the cone tucked into the collar
/// instead of dangling off the chin (the beard flap). The point cannot be sampled from the
/// palette itself: where the translation lives in each matrix depends on the block's palette
/// layout, and reading the wrong slots yields a pseudo-point rather than a position (observed
/// in-game as collapsed clusters parked ~0.6 m under the head, with blended vertices streaking
/// down toward them). Non-finite positions are dropped, leaving the previous target in place.
pub fn publish_collapse_target(position: glam::Vec3) {
    if !position.is_finite() {
        return;
    }
    for (slot, value) in COLLAPSE_TARGET.iter().zip(position.to_array()) {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
    COLLAPSE_TARGET_VALID.store(true, Ordering::Relaxed);
}

/// Publish the skeleton indices of the bones to collapse (HEAD plus the facial set), from the
/// character hook (the game thread owns the skeleton; the draws run on the render thread and only
/// load these). Index 0 (the root — also what a failed name lookup can resolve to) and
/// implausibly large values are dropped rather than risking a whole-body collapse.
pub fn publish_collapse_bones(indices: &[u32]) {
    let mut len = 0;
    for &index in indices {
        if index == 0 || index > 0x7FFF || len >= COLLAPSE_BONES.len() {
            continue;
        }
        COLLAPSE_BONES[len].store(index, Ordering::Relaxed);
        len += 1;
    }
    COLLAPSE_BONE_LEN.store(len, Ordering::Relaxed);
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacter::Draw_ADDRESS
)]
fn render_block_character_draw(
    this: *const RenderBlockCharacter,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let batches = unsafe { this.as_ref() }.map(|b| (b.m_SkinBatchesBegin, b.m_SkinBatchesEnd));
    unsafe { maybe_dump("Character::Draw", this as usize, batches, rc, info) };
    let hide = unsafe { should_hide_facial(rc, info) };
    HIDE_ACTIVE.with(|flag| flag.set(hide));
    RENDER_BLOCK_CHARACTER_DRAW
        .get()
        .unwrap()
        .call(this, rc, info);
    HIDE_ACTIVE.with(|flag| flag.set(false));
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacter::DrawZ_ADDRESS
)]
fn render_block_character_draw_z(
    this: *const RenderBlockCharacter,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let batches = unsafe { this.as_ref() }.map(|b| (b.m_SkinBatchesBegin, b.m_SkinBatchesEnd));
    unsafe { maybe_dump("Character::DrawZ", this as usize, batches, rc, info) };
    let hide = unsafe { should_hide_facial(rc, info) };
    HIDE_ACTIVE.with(|flag| flag.set(hide));
    RENDER_BLOCK_CHARACTER_DRAW_Z
        .get()
        .unwrap()
        .call(this, rc, info);
    HIDE_ACTIVE.with(|flag| flag.set(false));
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacterSkin::Draw_ADDRESS
)]
fn render_block_character_skin_draw(
    this: *const RenderBlockCharacterSkin,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let batches = unsafe { this.as_ref() }.map(|b| (b.m_SkinBatchesBegin, b.m_SkinBatchesEnd));
    unsafe { maybe_dump("CharacterSkin::Draw", this as usize, batches, rc, info) };
    let hide = unsafe { should_hide_facial(rc, info) };
    HIDE_ACTIVE.with(|flag| flag.set(hide));
    RENDER_BLOCK_CHARACTER_SKIN_DRAW
        .get()
        .unwrap()
        .call(this, rc, info);
    HIDE_ACTIVE.with(|flag| flag.set(false));
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacterSkin::DrawZ_ADDRESS
)]
fn render_block_character_skin_draw_z(
    this: *const RenderBlockCharacterSkin,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let batches = unsafe { this.as_ref() }.map(|b| (b.m_SkinBatchesBegin, b.m_SkinBatchesEnd));
    unsafe { maybe_dump("CharacterSkin::DrawZ", this as usize, batches, rc, info) };
    let hide = unsafe { should_hide_facial(rc, info) };
    HIDE_ACTIVE.with(|flag| flag.set(hide));
    RENDER_BLOCK_CHARACTER_SKIN_DRAW_Z
        .get()
        .unwrap()
        .call(this, rc, info);
    HIDE_ACTIVE.with(|flag| flag.set(false));
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacter::SetMatrixPalette_ADDRESS
)]
fn render_block_character_set_matrix_palette(
    this: *const RenderBlockCharacter,
    ctx: *mut HContext_t,
    matrices: *const Matrix3x4,
    batch: *const SkinBatch,
    register: u32,
) {
    let matrices = unsafe { spoofed_matrices(matrices, batch) }.unwrap_or(matrices);
    RENDER_BLOCK_CHARACTER_SET_MATRIX_PALETTE
        .get()
        .unwrap()
        .call(this, ctx, matrices, batch, register);
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacterSkin::SetMatrixPalette_ADDRESS
)]
fn render_block_character_skin_set_matrix_palette(
    this: *const RenderBlockCharacterSkin,
    ctx: *mut HContext_t,
    matrices: *const Matrix3x4,
    batch: *const SkinBatch,
    register: u32,
) {
    let matrices = unsafe { spoofed_matrices(matrices, batch) }.unwrap_or(matrices);
    RENDER_BLOCK_CHARACTER_SKIN_SET_MATRIX_PALETTE
        .get()
        .unwrap()
        .call(this, ctx, matrices, batch, register);
}

thread_local! {
    /// Set around the original `Draw`/`DrawZ` call for the local player's blocks in non-shadow
    /// passes; the palette hooks spoof only while it is up. Thread-local, not global: character
    /// draws run on multiple render worker threads (`CRBIInfo`'s constant-buffer bookkeeping is
    /// declared volatile for exactly that reason), and a global flag leaked from the player's
    /// draw into whatever unowned block drew concurrently — collapsing another skeleton's bones
    /// by index collision (observed in-game as a stretched holstered weapon).
    static HIDE_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// The scratch palette the spoof writes into, one per draw thread for the same reason.
    static PALETTE_BUF: std::cell::RefCell<[Matrix3x4; PALETTE_BUF_LEN]> =
        const { std::cell::RefCell::new([Matrix3x4 { m: [0.0; 12] }; PALETTE_BUF_LEN]) };
}

/// The skeleton indices of the bones to collapse; the first [`COLLAPSE_BONE_LEN`] entries are
/// valid.
static COLLAPSE_BONES: [AtomicU32; 16] = [const { AtomicU32::new(0) }; 16];
static COLLAPSE_BONE_LEN: AtomicUsize = AtomicUsize::new(0);

/// The NECK bone's model-space position as `f32` bits (see [`publish_collapse_target`]), plus a
/// flag for "published at least once".
static COLLAPSE_TARGET: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static COLLAPSE_TARGET_VALID: AtomicBool = AtomicBool::new(false);

/// The local player's root position pair, published by the character hook for the dump's
/// distance display; `None` until a local character exists.
static PLAYER_ROOT: parking_lot::Mutex<Option<(glam::Vec3, glam::Vec3)>> =
    parking_lot::Mutex::new(None);

/// The `CRBIInfo` pointers of the local player's model instances, slot-for-slot (zero = empty).
static PLAYER_RBI_INFOS: [AtomicUsize; PLAYER_MODEL_SLOTS] =
    [const { AtomicUsize::new(0) }; PLAYER_MODEL_SLOTS];

/// The model-instance slot holding the character's *body* model — the one whose skeleton the
/// collapse bone indices belong to. The other slots are attachments with their own skeletons
/// (the parachute among them), where the same numeric indices land on arbitrary bones.
const BODY_MODEL_SLOT: usize = 0;

/// Whether this draw belongs to the local player's body model in a non-shadow pass with the hide
/// enabled. Ownership is pointer identity: the draw's `info` is the owning model instance's
/// embedded `CRBIInfo`, matched against the body slot's published pointer. Fail-safe: any missing
/// piece (config off, shadow pass, nothing published) means "draw normally".
unsafe fn should_hide_facial(rc: *mut RenderContext, info: *const RBIInfo) -> bool {
    if !Config::lock_query(|c| c.camera.hide_head_draws) {
        return false;
    }
    let Some(rc_ref) = (unsafe { rc.as_ref() }) else {
        return false;
    };
    // Shadow passes draw everything: that is the whole point.
    if rc_ref.m_RenderStatus & 6 != 0 {
        return false;
    }
    let body = PLAYER_RBI_INFOS[BODY_MODEL_SLOT].load(Ordering::Relaxed);
    body != 0 && info as usize == body
}

const PALETTE_BUF_LEN: usize = 256;

/// How far the three basis vectors are from an orthonormal set: the layout whose 3x3 reading
/// scores lower is the matrix's real rotation (see the layout detection in [`spoofed_matrices`]).
fn ortho_error(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> f32 {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    (dot(x, x) - 1.0).abs()
        + (dot(y, y) - 1.0).abs()
        + (dot(z, z) - 1.0).abs()
        + dot(x, y).abs()
        + dot(x, z).abs()
        + dot(y, z).abs()
}

/// While the hide flag is up and this batch references a collapse bone, build a copy of the bone
/// matrices with the collapse bones replaced by a zero rotation and the published neck-point
/// translation, so their vertices contract to a point inside the collar. Returns `None` — draw
/// with the real palette — whenever the spoof does not apply.
unsafe fn spoofed_matrices(
    matrices: *const Matrix3x4,
    batch: *const SkinBatch,
) -> Option<*const Matrix3x4> {
    if !HIDE_ACTIVE.with(|flag| flag.get()) || matrices.is_null() {
        return None;
    }
    let b = unsafe { batch.as_ref() }?;
    if b.BatchToSkeletonLookup.is_null() {
        return None;
    }
    let len = COLLAPSE_BONE_LEN.load(Ordering::Relaxed);
    if len == 0 || !COLLAPSE_TARGET_VALID.load(Ordering::Relaxed) {
        return None;
    }
    let collapse = &COLLAPSE_BONES[..len];
    let is_collapse = |bone: usize| {
        collapse
            .iter()
            .any(|c| c.load(Ordering::Relaxed) as usize == bone)
    };

    // The upload reads `matrices[lookup[i]]` per batch slot, so the copy must span the batch's
    // highest referenced skeleton index; skip the spoof entirely if no collapse bone is used.
    let mut max_index = 0usize;
    let mut bones_counted = 0usize;
    let mut collapse_hits = 0usize;
    for i in 0..b.BatchSize.max(0) as usize {
        let bone = unsafe { *b.BatchToSkeletonLookup.add(i) } as u16 as usize;
        max_index = max_index.max(bone);
        if bone != 0 {
            bones_counted += 1;
            if is_collapse(bone) {
                collapse_hits += 1;
            }
        }
    }
    if collapse_hits == 0 || max_index >= PALETTE_BUF_LEN {
        return None;
    }
    // A batch that is mostly facial is face *gear* (eyewear and the like, sometimes also rigged
    // to a limb bone for handling animations): collapse every bone it references, or its
    // non-facial anchor holds part of it mid-body and the connecting triangles stretch into a rod
    // (observed in-game). Mixed batches — the body skin carries the face and the hands together —
    // keep the selective collapse so the hands stay.
    let collapse_whole_batch = collapse_hits * 2 >= bones_counted;

    PALETTE_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        for (i, slot) in buf[..=max_index].iter_mut().enumerate() {
            *slot = unsafe { *matrices.add(i) };
        }
        // The collapse target: the NECK bone's model-space position, published by the character
        // hook, so the stretch cones from vertices blended between collapsed and kept bones tuck
        // into the collar rather than dangling from each bone's own position (the beard flap).
        // The point deliberately does NOT come from the palette: its translation slots depend on
        // the per-block layout below, and sampling them without knowing it parks the collapsed
        // cluster at a phantom point (see [`publish_collapse_target`]).
        let target = {
            let [x, y, z] = &COLLAPSE_TARGET;
            [
                f32::from_bits(x.load(Ordering::Relaxed)),
                f32::from_bits(y.load(Ordering::Relaxed)),
                f32::from_bits(z.load(Ordering::Relaxed)),
            ]
        };
        // The palette layout differs per block (empirically: the body/facial blocks store four
        // 3-float columns with the translation last, the face-gear and one skin block store
        // three 4-float rows with the translation in the fourth element — the root bone's
        // identity matrix read column-wise as (0,1,0) at [9..11] was the tell). Detect the
        // layout per batch by orthonormality of the 3x3 under each reading, voting across the
        // referenced bones, then zero that layout's rotation and write the target into its
        // translation slots. Writing through the wrong layout puts the target into the rotation
        // and crushes the geometry toward the ground plane (observed in-game as downward
        // streaks).
        let mut col_error = 0.0f32;
        let mut row_error = 0.0f32;
        for i in 0..(b.BatchSize.max(0) as usize).min(8) {
            let bone = unsafe { *b.BatchToSkeletonLookup.add(i) } as u16 as usize;
            let m = &buf[bone].m;
            col_error += ortho_error([m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]);
            row_error += ortho_error([m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]);
        }
        let row_layout = row_error < col_error;
        let collapse_bone = |bone: usize, buf: &mut [Matrix3x4]| {
            if bone <= max_index {
                let m = &mut buf[bone].m;
                *m = [0.0; 12];
                if row_layout {
                    [m[3], m[7], m[11]] = target;
                } else {
                    m[9..].copy_from_slice(&target);
                }
            }
        };
        if collapse_whole_batch {
            // Including the root: face gear can anchor blend weights on bone 0 (leaving it kept
            // stretched a thin thread from the ground to the collar), and the upload is
            // per-batch, so zeroing the root entry in this copy touches nothing else.
            for i in 0..b.BatchSize.max(0) as usize {
                let bone = unsafe { *b.BatchToSkeletonLookup.add(i) } as u16 as usize;
                collapse_bone(bone, &mut buf[..]);
            }
        } else {
            for slot in collapse {
                collapse_bone(slot.load(Ordering::Relaxed) as usize, &mut buf[..]);
            }
        }
        if DUMP_DRAWS.load(Ordering::Relaxed) > 0 {
            tracing::info!(
                target: "render_block_dump",
                "  spoof batch={:?} size={} target=({:.2},{:.2},{:.2}) whole={} layout={} \
                 (col_err={:.2} row_err={:.2})",
                batch,
                b.BatchSize,
                target[0],
                target[1],
                target[2],
                collapse_whole_batch,
                if row_layout { "row" } else { "col" },
                col_error,
                row_error,
            );
        }
        // The pointer stays valid for the original call below: the thread-local outlives the
        // draw, and the engine copies the constants into the command stream at call time (its
        // own callers pass stack buffers).
        Some(buf.as_ptr())
    })
}

/// Log one draw's ground truth while [`DUMP_DRAWS`] is armed: the block class and pointer, the
/// pass status, the instance transform against the player roots, the collapse indices in use, and
/// every batch's skeleton lookup (capped). Draws further than 10 m from the player are skipped so
/// the dump stays focused.
unsafe fn maybe_dump(
    class: &str,
    block: usize,
    batches: Option<(*mut SkinBatch, *mut SkinBatch)>,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    if DUMP_DRAWS.load(Ordering::Relaxed) <= 0 {
        return;
    }
    let Some((rc_ref, info_ref)) = (unsafe { rc.as_ref() }).zip(unsafe { info.as_ref() }) else {
        return;
    };
    let Some((root_t0, root_t1)) = *PLAYER_ROOT.lock() else {
        return;
    };
    let mut matrix = Matrix4::default();
    unsafe { info_ref.GetMatrix(&mut matrix, rc_ref.m_TransformIndex as i32) };
    let translation = glam::Vec3::new(matrix.data[12], matrix.data[13], matrix.data[14]);
    let d0 = translation.distance(root_t0);
    let d1 = translation.distance(root_t1);
    if d0.min(d1) > 10.0 {
        return;
    }
    // Which of the player's model-instance slots (if any) this draw belongs to.
    let slot = PLAYER_RBI_INFOS
        .iter()
        .position(|slot| slot.load(Ordering::Relaxed) == info as usize && info as usize != 0)
        .map(|i| i as isize)
        .unwrap_or(-1);
    if DUMP_DRAWS.fetch_sub(1, Ordering::Relaxed) <= 0 {
        return;
    }
    let len = COLLAPSE_BONE_LEN.load(Ordering::Relaxed);
    let collapse: Vec<u32> = COLLAPSE_BONES[..len]
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .collect();
    let mut lines = String::new();
    if let Some((begin, end)) = batches {
        let mut batch = begin;
        while !batch.is_null() && batch < end {
            let b = unsafe { &*batch };
            let mut bones = Vec::new();
            if !b.BatchToSkeletonLookup.is_null() {
                for i in 0..(b.BatchSize.max(0) as usize).min(48) {
                    bones.push(unsafe { *b.BatchToSkeletonLookup.add(i) });
                }
            }
            lines.push_str(&format!(
                "\n  batch size={} indices={} offset={} bones={:?}",
                b.BatchSize, b.Size, b.Offset, bones
            ));
            batch = unsafe { batch.add(1) };
        }
    }
    tracing::info!(
        target: "render_block_dump",
        "{class} block={block:#x} status={:#x} tidx={} pos=({:.2},{:.2},{:.2}) d0={d0:.2} d1={d1:.2} slot={slot} collapse={collapse:?}{lines}",
        rc_ref.m_RenderStatus,
        rc_ref.m_TransformIndex,
        translation.x,
        translation.y,
        translation.z,
    );
}
