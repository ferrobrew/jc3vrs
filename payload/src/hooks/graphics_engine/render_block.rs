//! Character render-block detours: hide the local player's facial render blocks from every
//! non-shadow pass (issue #19).
//!
//! A character model is one render block per material, and the same block objects are drawn for
//! every pass — shadow, depth prepass, GBuffer — branching internally on
//! [`RenderContext::m_RenderStatus`]. Skipping the *draw call* for the player's facial blocks in
//! non-shadow passes therefore hides the whole head (face, eyes, and facial geometry — no
//! bone-scale hack, and none of the scale approach's unscaled-children leaks) while the shadow
//! passes keep drawing the untouched geometry: the shadow keeps its head, and it even follows the
//! player-driven head pose.
//!
//! Blocks are classified by their *skeleton bindings*: each [`SkinBatch`] carries a
//! batch-to-skeleton bone lookup, and only facial geometry is weighted to the facial bones
//! (published by the character hook from the live skeleton). Ownership is settled by proximity:
//! the draw's instance transform must sit within a couple of metres of the local player's head
//! anchor, which the character root always does and an NPC's practically never can.

use std::sync::atomic::{AtomicU32, Ordering};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        render_block::{RBIInfo, RenderBlockCharacter, RenderBlockCharacterSkin, SkinBatch},
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
}

/// Publish the skeleton indices of the facial classification bones, from the character hook (the
/// game thread owns the skeleton; the draws run on the render thread and only load these).
pub fn publish_facial_bones(indices: [u32; FACIAL_BONE_COUNT]) {
    for (slot, index) in FACIAL_BONE_INDICES.iter().zip(indices) {
        slot.store(index, Ordering::Relaxed);
    }
}

/// The number of facial bones used for classification (see the character hook's publish call).
pub const FACIAL_BONE_COUNT: usize = 3;

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockCharacter::Draw_ADDRESS
)]
fn render_block_character_draw(
    this: *const RenderBlockCharacter,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let batches = unsafe { this.as_ref() }.map(|b| (b.m_SkinBatchesBegin, b.m_SkinBatchesEnd));
    if unsafe { should_skip(rc, info, batches) } {
        return;
    }
    RENDER_BLOCK_CHARACTER_DRAW
        .get()
        .unwrap()
        .call(this, rc, info);
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
    if unsafe { should_skip(rc, info, batches) } {
        return;
    }
    RENDER_BLOCK_CHARACTER_DRAW_Z
        .get()
        .unwrap()
        .call(this, rc, info);
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
    if unsafe { should_skip(rc, info, batches) } {
        return;
    }
    RENDER_BLOCK_CHARACTER_SKIN_DRAW
        .get()
        .unwrap()
        .call(this, rc, info);
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
    if unsafe { should_skip(rc, info, batches) } {
        return;
    }
    RENDER_BLOCK_CHARACTER_SKIN_DRAW_Z
        .get()
        .unwrap()
        .call(this, rc, info);
}

/// The skeleton indices of the facial classification bones; `u32::MAX` until the character hook
/// publishes them.
static FACIAL_BONE_INDICES: [AtomicU32; FACIAL_BONE_COUNT] = [
    AtomicU32::new(u32::MAX),
    AtomicU32::new(u32::MAX),
    AtomicU32::new(u32::MAX),
];

/// How close (squared metres) the draw's instance transform must be to the local player's head
/// anchor to count as the player. The instance transform is the character root, which sits within
/// ~2 m of the head; another character's root cannot occupy the same space.
const OWNERSHIP_RADIUS_SQ: f32 = 2.5 * 2.5;

/// Whether this draw is one of the local player's facial blocks in a non-shadow pass. Fail-safe:
/// any missing piece (config off, shadow pass, no anchor yet, unpublished bones, null pointers)
/// means "draw normally".
unsafe fn should_skip(
    rc: *mut RenderContext,
    info: *const RBIInfo,
    batches: Option<(*mut SkinBatch, *mut SkinBatch)>,
) -> bool {
    if !Config::lock_query(|c| c.camera.hide_head_draws) {
        return false;
    }
    let Some((rc_ref, info_ref)) = (unsafe { rc.as_ref() }).zip(unsafe { info.as_ref() }) else {
        return false;
    };
    // Shadow passes draw everything: that is the whole point.
    if rc_ref.m_RenderStatus & 6 != 0 {
        return false;
    }
    let Some(anchor) = crate::headpose::anchor() else {
        return false;
    };

    // Ownership: the instance transform must sit at the local player.
    let mut matrix = Matrix4::default();
    unsafe { info_ref.GetMatrix(&mut matrix, rc_ref.m_TransformIndex as i32) };
    let translation = glam::Vec3::new(matrix.data[12], matrix.data[13], matrix.data[14]);
    if translation.distance_squared(anchor) > OWNERSHIP_RADIUS_SQ {
        return false;
    }

    // Classification: facial geometry is the geometry weighted to a facial bone.
    let facial = FACIAL_BONE_INDICES
        .each_ref()
        .map(|i| i.load(Ordering::Relaxed));
    if facial.contains(&u32::MAX) {
        return false;
    }
    let Some((begin, end)) = batches else {
        return false;
    };
    let mut batch = begin;
    while !batch.is_null() && batch < end {
        let b = unsafe { &*batch };
        if !b.BatchToSkeletonLookup.is_null() {
            for i in 0..b.BatchSize.max(0) as usize {
                let bone = unsafe { *b.BatchToSkeletonLookup.add(i) } as u16 as u32;
                if facial.contains(&bone) {
                    return true;
                }
            }
        }
        batch = unsafe { batch.add(1) };
    }
    false
}
