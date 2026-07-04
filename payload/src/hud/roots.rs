//! The render-root partition: split the HUD movie's render tree across extra `TreeRoot`s in its
//! own render context, so each depth layer renders into its own texture at full rate (issue #14).
//!
//! Rendering the movie several times with visibility masks cannot work (captures are only safe on
//! the game thread; see `docs/issue-08-14-hud-overlays-and-depth.md`), and refreshing one layer
//! per frame undersamples world-to-screen'd content visibly. This mechanism reshapes the *render*
//! tree instead: the layer containers' render nodes move from the movie's main root into roots we
//! create in the same context, and the render detour draws each root into its own texture from
//! the frame's single vanilla capture -- full rate, zero extra captures, no masking, and the
//! display side (AS3, hit-testing) untouched.
//!
//! The display side addresses its children's render nodes by *cached numeric index*
//! (`GFx::DisplayList::TreeIndex`), so a moved node is swapped with an empty **tombstone**
//! container at the same index: the array length and every sibling's cached index stay valid.
//! Node-local updates (matrix, visibility, alpha -- the game's per-frame POI writes) reach the
//! real node wherever it lives; only structural ops are index-based and hit the tombstone. Those
//! are reconciled at the capture seam each frame: a tombstone whose parent went null was removed
//! by the display side (a POI pool despawn), so the real node leaves its root too; a new unknown
//! child of the HUD clip's container is a pool spawn and gets adopted into the markers root. Pool
//! spawns happen during `Advance`/`UpdatePOIs`, before the capture seam, so adoption lands in the
//! same frame's capture.
//!
//! All structural work runs on the game update thread at the [`MovieImpl::Capture`] seam, under
//! the deferred render lock, exactly like the display side's own mutations. The render side only
//! reads the root entry pointers; entries are the engine's own cross-thread mechanism, and the
//! deferred render lock excludes teardown from a concurrent render.

use jc3gi::ui::{
    scaleform::{MovieImpl, RTHandle, TreeContainer, TreeNode, TreeRoot, Value},
    ui_manager::UIManager,
};
use windows::{
    Win32::{
        Graphics::Direct3D11::{D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DeviceContext},
        System::Threading::{EnterCriticalSection, GetCurrentThreadId, LeaveCriticalSection},
    },
    core::Interface as _,
};

use super::split::{ClipHandle, HudLayer, LAYER_COUNT, LayerViews};

/// The number of extra roots (every layer beyond the static one, which stays in the movie's own
/// root).
pub const EXTRA_ROOT_COUNT: usize = LAYER_COUNT - 1;

/// The live partition, `None` while inactive. All mutation happens on the game update thread at
/// the capture seam; the render side briefly locks to copy the root pointers.
pub(crate) static PARTITION: parking_lot::Mutex<Option<Partition>> = parking_lot::Mutex::new(None);

/// The partition state: the extra roots, the shared parent container, and every moved node.
pub(crate) struct Partition {
    /// The extra roots, indexed by [`HudLayer`] minus one (markers, center).
    roots: [*mut TreeRoot; EXTRA_ROOT_COUNT],
    /// The HUD clip's own render container (the parent every layer container and pool clip
    /// shares).
    hud_container: *mut TreeContainer,
    /// Every node moved out of [`hud_container`](Partition::hud_container), with its tombstone.
    moved: Vec<MovedNode>,
    /// The static layer's nodes (left in place): known children that must not be adopted.
    known_static: Vec<*mut TreeNode>,
}

/// One node moved into an extra root, and the tombstone holding its display-side index.
struct MovedNode {
    /// The real render node, now a child of `roots[root]`.
    node: *mut TreeNode,
    /// The empty container occupying the node's index in the original parent. We hold a
    /// reference, so the pointer stays valid even after the display side removes it.
    tombstone: *mut TreeContainer,
    /// Which extra root the node lives in.
    root: usize,
}

// SAFETY: the raw entry pointers are only dereferenced on the game update thread at the capture
// seam (mutation) or copied as opaque values for the render side (which the engine's entry
// system is built for); the mutex serializes access.
unsafe impl Send for Partition {}

/// The extra roots' entry pointers for the render detour, or `None` while the partition is not
/// live.
pub fn render_roots() -> Option<[*mut TreeRoot; EXTRA_ROOT_COUNT]> {
    PARTITION.lock().as_ref().map(|p| p.roots)
}

/// Whether the partition is live (the draw side composites per-layer textures while it is).
pub fn live() -> bool {
    PARTITION.lock().is_some()
}

/// The capture-seam step (game update thread, deferred render lock held, display writers
/// quiescent). While `active`, builds the partition when possible and reconciles it against the
/// display side's structural changes; while not, tears it down (restoring every node).
///
/// # Safety
/// Must be called from the [`MovieImpl::Capture`] detour on the main UI movie, before the
/// original runs.
pub unsafe fn on_capture(movie: &mut MovieImpl, active: bool) {
    let mut partition = PARTITION.lock();
    // SAFETY (whole body): capture-seam threading per the function contract; every entry pointer
    // held by the partition is kept alive by a reference we own.
    unsafe {
        if !active {
            if let Some(mut p) = partition.take() {
                teardown(&mut p);
            }
            return;
        }
        if partition.is_none() {
            *partition = build(movie);
        }
        let Some(p) = partition.as_mut() else {
            return;
        };
        // Mirror the movie's viewport and stage matrix onto our roots (both writes self-compare
        // upstream or are cheap), so resizes track.
        for root in p.roots {
            (*root).SetViewport(movie.Viewport.as_ptr());
            root.cast::<TreeNode>()
                .as_mut()
                .unwrap_unchecked()
                .SetMatrix(movie.ViewportMatrix.as_ptr());
        }
        reconcile(p);
    }
}

/// Tear the partition down from outside the capture seam (still on the game update thread, e.g.
/// ahead of a clip-handle rediscovery or shutdown). Safe because every display writer runs on
/// this thread too.
pub fn teardown_now() {
    if let Some(mut p) = PARTITION.lock().take() {
        // SAFETY: game update thread; see above.
        unsafe { teardown(&mut p) };
    }
}

/// Build the partition: resolve every layer container's render node from the clip-handle
/// registry, create the extra roots, and move the marker/center containers (plus any existing
/// pool clips) behind tombstones. Returns `None` (leaving no side effects beyond empty roots)
/// when the handles or nodes are not available yet.
unsafe fn build(movie: &mut MovieImpl) -> Option<Partition> {
    let mut handles = super::split::CLIP_HANDLES.lock();
    let handles = handles.as_mut()?;

    // SAFETY (whole body): capture-seam threading per on_capture's contract.
    unsafe {
        // Resolve the layer containers' nodes. The static layer's nodes are only recorded (they
        // stay in place); the marker/center containers must all resolve, or the partition would
        // silently render those layers empty.
        let known_static: Vec<*mut TreeNode> = handles.containers[HudLayer::Static as usize]
            .iter()
            .filter_map(|handle| clip_render_node(handle))
            .collect();
        let mut layer_nodes: [Vec<*mut TreeNode>; EXTRA_ROOT_COUNT] = Default::default();
        for (slot, layer) in [HudLayer::Markers, HudLayer::Center]
            .into_iter()
            .enumerate()
        {
            for handle in &handles.containers[layer as usize] {
                layer_nodes[slot].push(clip_render_node(handle)?);
            }
        }

        // The shared parent: every layer container is a direct child of the HUD clip.
        let hud_container = (**layer_nodes[0].first()?).pParent.cast::<TreeContainer>();
        if hud_container.is_null() {
            return None;
        }
        let same_parent = layer_nodes
            .iter()
            .flatten()
            .all(|&n| (*n).pParent.cast::<TreeContainer>() == hud_container);
        if !same_parent {
            tracing::warn!(
                "hud roots: the layer containers do not share one parent; not partitioning"
            );
            return None;
        }

        // The container must be part of the movie's *live* tree: the game re-attaches the HUD
        // clips around menu transitions, and handles resolved against the previous attachment
        // point still resolve to live-but-detached objects. Moving those would double-parent
        // them once the game re-inserts them (which hangs the renderer's cache reconciliation),
        // so refuse and wait for a fresh discovery.
        let mut walk = hud_container.cast::<TreeNode>();
        for _ in 0..64 {
            let parent = (*walk).pParent;
            if parent.is_null() {
                break;
            }
            walk = parent;
        }
        if walk != movie.pRenderRoot.cast::<TreeNode>() {
            tracing::warn!(
                "hud roots: the HUD container is not attached to the live render root; not \
                 partitioning"
            );
            return None;
        }

        let context = &mut movie.RenderContext;
        let roots = [context.CreateEntryTreeRoot(), context.CreateEntryTreeRoot()];
        if roots.iter().any(|r| r.is_null()) {
            tracing::error!("hud roots: TreeRoot creation failed");
            for root in roots {
                if !root.is_null() {
                    release_entry(root.cast());
                }
            }
            return None;
        }

        let mut p = Partition {
            roots,
            hud_container,
            moved: Vec::new(),
            known_static,
        };
        for (slot, nodes) in layer_nodes.iter().enumerate() {
            for &node in nodes {
                move_node(&mut p, movie, node, slot);
            }
        }
        tracing::info!(
            "hud roots: partition live ({} nodes moved behind tombstones)",
            p.moved.len()
        );
        Some(p)
    }
}

/// Per-frame reconciliation: release nodes whose tombstones the display side removed (pool
/// despawns), and adopt new children of the HUD container (pool spawns) into the markers root.
unsafe fn reconcile(p: &mut Partition) {
    // SAFETY (whole body): capture-seam threading per on_capture's contract.
    unsafe {
        // Despawns and reclaims. A tombstone whose parent went null was removed by the display
        // side (a pool despawn): drop the real node from its root; the display object's own
        // reference decides its lifetime. A moved node whose parent is no longer our root was
        // re-inserted *somewhere* by the display side (`Insert` overwrites `pParent`): it is
        // double-parented, which the renderer's lockstep cache reconciliation never converges on
        // (an infinite loop under the deferred render lock, hanging the game). Give it back:
        // remove it from our root's array and restore the display side's parent (our removal
        // nulls it). The tombstone reference is dropped either way -- a still-placed tombstone
        // must stay put (pulling it would shift the display side's cached indices), renders
        // nothing, and the display side's own eventual removal destroys it.
        let mut despawned = 0usize;
        let mut reclaimed = 0usize;
        let mut index = 0;
        while index < p.moved.len() {
            let entry = &p.moved[index];
            let tombstone_gone = (*entry.tombstone.cast::<TreeNode>()).pParent.is_null();
            let display_parent = (*entry.node).pParent;
            let reparented = display_parent != p.roots[entry.root].cast::<TreeNode>();
            if !tombstone_gone && !reparented {
                index += 1;
                continue;
            }
            let entry = p.moved.swap_remove(index);
            remove_from_root(p.roots[entry.root], entry.node);
            if reparented && !display_parent.is_null() {
                // Our removal nulled the parent the display side just set; restore it.
                (*entry.node).pParent = display_parent;
                reclaimed += 1;
            } else {
                despawned += 1;
            }
            release_entry(entry.tombstone.cast());
        }

        // Spawns: any child of the HUD container we do not know is a fresh pool clip; adopt it
        // into the markers root. Collect first: moving mutates the child array. Nodes still in
        // `moved` can no longer appear here (the reclaim pass above dropped any the display side
        // took back), but never adopt one regardless -- a double insert is the hang above.
        let container = p.hud_container.as_mut().unwrap_unchecked();
        let mut fresh = Vec::new();
        for i in 0..container.GetSize() {
            let child = container.GetAt(i);
            if child.is_null()
                || p.known_static.contains(&child)
                || p.moved
                    .iter()
                    .any(|m| m.tombstone.cast::<TreeNode>() == child || m.node == child)
            {
                continue;
            }
            fresh.push(child);
        }
        let adopted = fresh.len();
        for node in fresh {
            adopt(p, node);
        }
        if adopted != 0 || despawned != 0 || reclaimed != 0 {
            tracing::info!(
                "hud roots: reconciled {adopted} adoption(s), {despawned} despawn(s), \
                 {reclaimed} reclaim(s) ({} moved total)",
                p.moved.len()
            );
        }
    }
}

/// Adopt a fresh pool clip into the markers root (tombstone swap, like the initial move).
unsafe fn adopt(p: &mut Partition, node: *mut TreeNode) {
    // The context is reachable from any entry's page, but the movie's own context pointer is
    // what the creation helpers take; thread it through the global instead.
    // SAFETY: capture seam; the UI manager and movie are live (we are inside its capture).
    unsafe {
        let Some(manager) = jc3gi::ui::ui_manager::UIManager::get() else {
            return;
        };
        let Some(movie) = manager.m_Movie.as_mut() else {
            return;
        };
        move_node(p, movie, node, HudLayer::Markers as usize - 1);
    }
}

/// Swap `node` (a child of the HUD container) with a fresh tombstone and append it to
/// `roots[slot]`.
unsafe fn move_node(p: &mut Partition, movie: &mut MovieImpl, node: *mut TreeNode, slot: usize) {
    // SAFETY (whole body): capture-seam threading per on_capture's contract.
    unsafe {
        let container = p.hud_container.as_mut().unwrap_unchecked();
        let Some(index) = find_child(container, node) else {
            return;
        };
        let tombstone = movie.RenderContext.CreateEntryTreeContainer();
        if tombstone.is_null() {
            return;
        }
        // Keep the node alive across the remove (Remove drops a reference), and keep our own
        // reference on the tombstone so its pointer stays readable after a display-side removal.
        (*node).RefCount += 1;
        container.Remove(index, 1);
        container.Insert(index, tombstone.cast());
        let root = p.roots[slot]
            .cast::<TreeContainer>()
            .as_mut()
            .unwrap_unchecked();
        root.Insert(root.GetSize(), node);
        (*node).RefCount -= 1;
        p.moved.push(MovedNode {
            node,
            tombstone,
            root: slot,
        });
    }
}

/// Restore every moved node to its tombstone's position (when the display side still has the
/// tombstone), drop the tombstones and roots, and leave the tree exactly as the display side
/// believes it is.
unsafe fn teardown(p: &mut Partition) {
    // SAFETY (whole body): game update thread; the deferred render lock is either held (capture
    // seam) or no render is in flight (shutdown path).
    unsafe {
        let container = p.hud_container.as_mut().unwrap_unchecked();
        for entry in p.moved.drain(..) {
            let tombstone_node = entry.tombstone.cast::<TreeNode>();
            let display_parent = (*entry.node).pParent;
            let reparented = display_parent != p.roots[entry.root].cast::<TreeNode>();
            if reparented {
                // The display side already re-owns the node elsewhere; detach it from our array
                // and restore the parent our removal nulls.
                remove_from_root(p.roots[entry.root], entry.node);
                if !display_parent.is_null() {
                    (*entry.node).pParent = display_parent;
                }
            } else if (*tombstone_node).pParent.is_null() {
                // The display side already dropped this child; just detach the real node.
                remove_from_root(p.roots[entry.root], entry.node);
            } else if let Some(index) = find_child(container, tombstone_node) {
                (*entry.node).RefCount += 1;
                remove_from_root(p.roots[entry.root], entry.node);
                container.Remove(index, 1);
                container.Insert(index, entry.node);
                (*entry.node).RefCount -= 1;
            }
            release_entry(entry.tombstone.cast());
        }
        for root in p.roots {
            release_entry(root.cast());
        }
        tracing::info!("hud roots: partition torn down");
    }
}

/// Find `node`'s index in `container`'s active child array.
unsafe fn find_child(container: &TreeContainer, node: *mut TreeNode) -> Option<u64> {
    // SAFETY: capture seam; reads the active snapshot.
    unsafe { (0..container.GetSize()).find(|&i| container.GetAt(i) == node) }
}

/// Remove `node` from `root`'s child array, if present.
unsafe fn remove_from_root(root: *mut TreeRoot, node: *mut TreeNode) {
    // SAFETY: capture seam.
    unsafe {
        let container = root.cast::<TreeContainer>().as_mut().unwrap_unchecked();
        if let Some(index) = find_child(container, node) {
            container.Remove(index, 1);
        }
    }
}

/// Drop our reference on an entry, destroying it at zero.
unsafe fn release_entry(entry: *mut TreeNode) {
    // SAFETY: capture seam; we hold the reference being dropped.
    unsafe {
        (*entry).RefCount -= 1;
        if (*entry).RefCount == 0 {
            (*entry).DestroyHelper();
        }
    }
}

/// Resolve a clip handle's render node: the managed display-object [`Value`]'s AS3 object at
/// `mValue`, its `DisplayObjectBase` at `+0x88` (guarded by the traits check `GetDisplayInfo`
/// itself performs on `+0x28`), then `GetRenderNode`.
unsafe fn clip_render_node(handle: &ClipHandle) -> Option<*mut TreeNode> {
    // SAFETY: capture seam; the handle's value is pinned and managed.
    unsafe {
        let value: &Value = handle.value.as_deref()?;
        let object = value.mValue as *const u8;
        if object.is_null() {
            return None;
        }
        let traits = *(object.add(0x28) as *const *const u8);
        if traits.is_null() {
            return None;
        }
        let type_id = *(traits.add(0x78) as *const u32);
        let flags = *traits.add(0x70);
        if type_id.wrapping_sub(24) >= 12 || flags & 0x20 != 0 {
            return None;
        }
        let display_object =
            *(object.add(0x88) as *const *const jc3gi::ui::scaleform::DisplayObjectBase);
        let node = display_object.as_ref()?.GetRenderNode();
        (!node.is_null()).then_some(node)
    }
}

/// Render the partitioned HUD: a reimplementation of `CUIManager::Render`'s body (verified
/// against its decompilation) that draws the movie's main root and then each extra root into its
/// own texture, all from the frame's single capture. Mirrors `RenderOffScreenTextures`' per-target
/// pattern for the extra roots: a fresh `RenderTarget` from the layer's views, `PushRenderTarget`,
/// draw, `PopRenderTarget`, release. Returns false (having done nothing) when a precondition is
/// missing, so the caller can fall back to the original.
///
/// # Safety
/// Must be called from the detour on `CUIManager::Render`, on the UI render worker, with `this`
/// being the detour's own argument.
pub unsafe fn render_partitioned(this: *mut UIManager, views: &LayerViews) -> bool {
    let Some(roots) = render_roots() else {
        return false;
    };
    // SAFETY (whole body): `this` is the live UI manager inside its own Render call path; the
    // deferred render lock (the engine's PreRender/Render exclusion) is held across everything,
    // which also excludes a concurrent partition teardown (it runs under the same lock inside
    // PreRender).
    unsafe {
        let Some(manager) = this.as_mut() else {
            return false;
        };
        if !manager.m_RenderReady || !manager.m_RenderActive || !manager.m_RenderingEnabled {
            return false;
        }
        let lock = manager.m_DeferredRenderLock;
        let (Some(hal), Some(movie), false) = (
            manager.m_RenderHAL.as_mut(),
            manager.m_Movie.as_mut(),
            lock.is_null(),
        ) else {
            return false;
        };
        if hal.pDeviceContext.is_null() {
            return false;
        }

        EnterCriticalSection(lock as *mut _);

        // The original's own preamble: stamp the render thread onto the texture manager and the
        // command queue, and drain the queue.
        let thread_id = GetCurrentThreadId();
        if let Some(texture_manager) = manager.m_TextureManager.as_mut() {
            texture_manager.RenderThreadId = thread_id;
        }
        if let Some(queue) = manager.m_ThreadCommandQueue.as_mut() {
            queue.m_RenderThreadId = thread_id;
            queue.Execute();
        }

        // Every texture redraws every call, so clear them all up front on the HAL's own device
        // context (ordered with the draws it is about to issue).
        let device_context =
            std::mem::ManuallyDrop::new(ID3D11DeviceContext::from_raw(hal.pDeviceContext));
        for (rtv, dsv) in &views.views {
            device_context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);
            device_context.ClearDepthStencilView(
                dsv,
                (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
                1.0,
                0,
            );
        }

        // The main root (now holding only the static layer) into the display target, exactly as
        // the original does: consume the frame's capture and draw.
        hal.SetRenderTarget(manager.m_RenderBuffer.cast(), true);
        hal.BeginFrame();
        hal.BeginScene();
        let notify = hal.GetContextNotify();
        let mut handle = RTHandle {
            pData: std::ptr::null_mut(),
        };
        if let Some(display_handle) = movie.GetDisplayHandle().as_ref() {
            handle.pData = display_handle.pData;
            if let Some(data) = handle.pData.as_ref() {
                data.AddRef();
            }
        }
        if handle.NextCapture(notify, 0) {
            let entry = handle.GetRenderEntry();
            if !entry.is_null() {
                hal.Draw(entry);
            }
        }
        handle.Destruct();
        hal.EndScene();

        // The extra roots, each into its own texture (the off-screen pattern).
        for (slot, root) in roots.into_iter().enumerate() {
            let (rtv, dsv) = &views.views[slot + 1];
            let (width, height) = views.sizes[slot + 1];
            let target = hal.CreateRenderTarget(rtv.as_raw(), dsv.as_raw());
            let Some(target_ref) = target.as_mut() else {
                continue;
            };
            let frame_rect = [0.0f32, 0.0, width as f32, height as f32];
            let clear_color: u32 = 0;
            hal.PushRenderTarget(target, 0, frame_rect.as_ptr(), &clear_color);
            hal.BeginScene();
            hal.Draw(root);
            hal.EndScene();
            hal.PopRenderTarget(0);
            target_ref.Release();
        }

        hal.EndFrame();
        hal.PopRenderTarget(0);

        LeaveCriticalSection(lock as *mut _);
    }
    true
}
