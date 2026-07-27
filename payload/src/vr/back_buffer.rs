//! Mod-owned back buffer: substitute a mod-allocated render target for the engine's swapchain-derived
//! one, so the render resolution and the swapchain size stop being the same number.
//!
//! See `docs/mod/swapchain-ownership.md` for the full design and the reverse-engineering behind it.
//! The short version: the engine builds `m_BackBufferLinear` as a *format alias of DXGI back buffer
//! 0*, so resizing the scene to the per-eye render resolution drags the swapchain along with it, and
//! the desktop present then rescales the whole frame onto a window of a different size and shape.
//! Substituting a mod-owned texture for the three swapchain-derived objects breaks that link.
//!
//! ## Ownership, and why the order matters
//!
//! [`Graphics::DestroySurface`](jc3gi::graphics_engine::surface::DestroySurface) has no already-freed
//! guard on this build: destroying a surface twice is an unguarded use-after-free that corrupts
//! silently and surfaces somewhere unrelated, later. So there is exactly one ownership rule here, and
//! it is not negotiable:
//!
//! - The three substitute objects (two render setups and the surface) live in the **engine's own
//!   fields**, and the engine's `DestroyRenderSetups` frees them correctly, because they are
//!   engine-allocated objects sitting where engine-allocated objects belong. The mod must never hold
//!   a second pointer to any of them, and must never free them itself.
//! - The **backing texture** is the one object the engine knows nothing about. The mod owns that
//!   handle alone, and frees it exactly once, after the engine has been told to stop using it.
//!
//! Every transition therefore clears [`owned`] *before* the resize that rebuilds the engine's
//! originals, never after: clearing it afterwards would leave the engine bound to a texture about to
//! be freed.
//!
//! ## State
//!
//! Substitution is live only while it buys something: [`crate::vr::resolution`] takes ownership only
//! while it is driving a render size of its own -- an XR session with `vr.native_resolution` on --
//! and only with `vr.own_back_buffer` set (`docs/mod/swapchain-ownership.md` §8). Outside that, every
//! path here is inert and the engine's own objects stand untouched.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use jc3gi::graphics_engine::{
    graphics_engine::{GraphicsEngine, HDevice_t, HRenderSetup_t, HTexture_t},
    surface::{
        Create2DTexture, Create2DTextureParams, CreateRenderSetup, CreateRenderSetupParams,
        Destroy2DTexture, DestroyRenderSetup, DestroySurface, GetRenderTarget, MultisampleFormat,
        PoolType, SurfaceFormat, UsageType,
    },
    texture::Texture,
};
use parking_lot::Mutex;

/// The mod-owned backing texture and the size it was created at.
///
/// Only the texture is here. The surface and the two render setups built over it live in the engine's
/// own fields, and the engine's `DestroyRenderSetups` frees them -- holding a second pointer to any of
/// them here is exactly the double-free the module docs warn about.
struct Backing {
    texture: *mut HTexture_t,
    size: (u32, u32),
}

/// The engine's handles are plain pointers into engine-owned memory, reached only on the render
/// thread under the drained idle context; the mutex is what makes the handoff between that thread and
/// the game thread's transitions sound.
unsafe impl Send for Backing {}

/// The mod-owned backing texture, or `None` when nothing is held.
static BACKING: Mutex<Option<Backing>> = Mutex::new(None);

/// Whether the mod's substitute objects are currently installed in the engine's fields.
///
/// Distinct from [`OWNED`], which is intent: ownership can be released while a substitution is still
/// installed, and stays that way until the next `CreateRenderSetups` rebuilds the engine's own
/// objects. The backing texture may only be freed once this is false -- that is the difference
/// between freeing memory the engine has stopped using and freeing memory it is still rendering into.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Set while the mod is deliberately driving a real swapchain resize, so the `Graphics::ResizeBuffers`
/// substitute stands aside and lets the original run (see [`sync_swapchain_to_window`]).
static BYPASS_RESIZE_SUBSTITUTE: AtomicBool = AtomicBool::new(false);

/// The name the substitute texture is created under. The resource tracking that consumed it is
/// compiled out of this build, so it is only ever seen by someone reading a memory dump.
const BACKING_NAME: &std::ffi::CStr = c"VrBackBuffer";

/// Whether the mod currently owns the engine's back-buffer objects.
///
/// Read by both hook gates on the render thread and written by the session transitions on the game
/// thread, so it is an atomic rather than living in [`STATE`]: the gates must never block, and a hook
/// that took the state lock could deadlock against a transition holding it across an `ApplyResize`.
static OWNED: AtomicBool = AtomicBool::new(false);

/// Whether the mod owns the engine's back-buffer objects right now.
///
/// The gate for both hooks. While false, `CreateRenderSetups`' epilogue performs no substitution and
/// `Graphics::ResizeBuffers` does its real job, so the engine behaves exactly as it does without the
/// mod -- which is what makes eject and the device-reset paths safe.
pub fn owned() -> bool {
    OWNED.load(Ordering::Acquire)
}

/// Whether the mod's substitute objects are currently installed in the engine's fields.
///
/// Distinct from [`owned`]: ownership can be released while a substitution is still installed, and
/// only the next `CreateRenderSetups` (driven by an `ApplyResize`) clears this. Callers that need the
/// engine's own aliases rebuilt -- for instance a shutdown restore -- must force that resize whenever
/// this is true, not just when the display size actually changes.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Whether the `Graphics::ResizeBuffers` substitute should stand aside for this call, letting the
/// real function resize the DXGI buffers. Only true inside [`sync_swapchain_to_window`].
pub fn resize_substitute_bypassed() -> bool {
    BYPASS_RESIZE_SUBSTITUTE.load(Ordering::Acquire)
}

/// Bring the DXGI swapchain to the window's client size, once the engine no longer renders into it.
///
/// Taking ownership stops the swapchain *following* the render size, but it does not move a swapchain
/// that is already oversized -- which is exactly the state when the flag is switched on mid-session,
/// with the engine long since resized to the per-eye resolution. Without this the substitution
/// installs correctly and changes nothing visible, because the mirror still presents through an
/// oversized buffer.
///
/// Safe to do only *because* of the substitution: the engine's alias on back buffer 0 is gone, so
/// nothing outside the mod references it, and `IDXGISwapChain::ResizeBuffers` can succeed. The
/// mirror's cached views are dropped first for the same reason -- they are the mod's own references
/// to buffer 0, and it rebuilds them lazily on the next frame.
///
/// A no-op unless ownership is live, the substitution is installed, and the sizes actually differ.
///
/// **Must run with the draw thread drained**, from the frame top on the game thread. That is what
/// makes [`BYPASS_RESIZE_SUBSTITUTE`] sound: it is a process-global flag guarding a window in which
/// no other thread may enter the `Graphics::ResizeBuffers` detour. If an `ApplyResize` on the render
/// thread ever overlapped that window it would take the real resize and drag the swapchain to the
/// render size, silently undoing the feature with nothing logged.
pub fn sync_swapchain_to_window() {
    if !owned() || BACKING.lock().is_none() {
        return;
    }
    // SAFETY: game thread at the frame top; every hop is null-guarded.
    let Some(engine) = (unsafe { GraphicsEngine::get() }) else {
        return;
    };
    let Some(device) = (unsafe { engine.m_Device.as_ref() }) else {
        return;
    };
    let Some(back_buffer) = (unsafe { device.m_BackBuffer.as_ref() }) else {
        return;
    };
    let current = (
        u32::from(back_buffer.m_Width),
        u32::from(back_buffer.m_Height),
    );
    let Some(window) = super::window::client_size() else {
        return;
    };
    if current == window {
        return;
    }

    // Drop the mirror's render-target view over buffer 0 before the resize; it is rebuilt lazily.
    super::mirror::teardown();

    // The real `ResizeBuffers` writes the device info as well as the DXGI buffers -- it is the same
    // write Hook B stands in for. Here that is wrong: the device info must keep meaning "the render
    // size", which is precisely what the substitution decoupled it from the swapchain to say. Left
    // clobbered, the render-size driver sees the size collapse to the window's, requests the per-eye
    // size again, and every sync sets off another resize-and-resubstitute round trip.
    let render_size = (
        device.m_DeviceInfo.m_DisplayWidth,
        device.m_DeviceInfo.m_DisplayHeight,
        device.m_DeviceInfo.m_DisplayRatio,
    );

    BYPASS_RESIZE_SUBSTITUTE.store(true, Ordering::Release);
    // SAFETY: the engine no longer aliases the swapchain (the substitution replaced its back-buffer
    // objects), and the mod's own views were just released, so no reference to buffer 0 outstands.
    let ok = unsafe {
        jc3gi::graphics_engine::device::ResizeBuffers(
            engine.m_Device.cast::<HDevice_t>(),
            window.0,
            window.1,
        )
    };
    BYPASS_RESIZE_SUBSTITUTE.store(false, Ordering::Release);

    // Put the render size back. `device->m_BackBuffer`'s own dimensions keep the new swapchain size,
    // which is what they should now report.
    // SAFETY: the game thread owns the device here; the pointer was null-checked above.
    if let Some(device) = unsafe { engine.m_Device.as_mut() } {
        device.m_DeviceInfo.m_DisplayWidth = render_size.0;
        device.m_DeviceInfo.m_DisplayHeight = render_size.1;
        device.m_DeviceInfo.m_DisplayRatio = render_size.2;
    }

    if ok {
        tracing::info!(
            target: "vr",
            from_width = current.0,
            from_height = current.1,
            to_width = window.0,
            to_height = window.1,
            "back buffer: resized the swapchain to the window",
        );
    } else {
        tracing::warn!(
            target: "vr",
            width = window.0,
            height = window.1,
            "back buffer: the swapchain resize to the window size failed; the mirror stays scaled",
        );
    }
}

/// Take ownership: from the next `ApplyResize` onward, the engine renders into a mod-owned texture
/// and the swapchain stops following the render size.
///
/// Sets the flag only; the substitution itself happens in the `CreateRenderSetups` epilogue, which
/// the caller triggers through the existing deferred resize path
/// ([`crate::vr::resolution`]). Ordering is the point: the flag must be set *before* the resize that
/// the substitution rides on.
pub fn enable() {
    if OWNED.swap(true, Ordering::AcqRel) {
        return;
    }
    tracing::info!(target: "vr", "back buffer: mod ownership enabled");
}

/// Release ownership, so the next `ApplyResize` rebuilds the engine's own alias and render setups
/// over the live swapchain.
///
/// Clears the flag only, and must be called *before* that resize is requested -- see the module
/// ownership note. Returns whether ownership was actually held, so the caller can skip the restore
/// work when there was nothing to restore.
pub fn disable() -> bool {
    let was_owned = OWNED.swap(false, Ordering::AcqRel);
    if was_owned {
        tracing::info!(target: "vr", "back buffer: mod ownership released");
    }
    was_owned
}

/// Free the mod-owned backing texture, after the engine has been resized off it.
///
/// The engine's `DestroyRenderSetups` frees the two substitute setups and the substitute surface,
/// because those live in its own fields; this handle is the one object it does not know about. Safe
/// to call when nothing is held -- it is a no-op then.
///
/// # Safety
/// The engine must no longer reference the texture: [`disable`] must have been called *and* the
/// subsequent `ApplyResize` must have completed, so the engine's own objects are back in place.
/// Calling this while the engine still holds the texture leaves it rendering into freed memory.
pub unsafe fn release_backing_texture(engine: &mut GraphicsEngine) {
    if INSTALLED.load(Ordering::Acquire) {
        // The engine's fields still point at a surface and render setups built over this texture,
        // because the `ApplyResize` that would have rebuilt its own never ran. Leaking the texture is
        // the strictly better outcome: freeing it now leaves the composite writing into freed memory,
        // and `Graphics::DestroySurface` has no already-freed guard to catch what follows.
        tracing::warn!(
            target: "vr",
            "back buffer: the substitution is still installed; retaining the backing texture. The \
             engine's m_BackBufferLinear, m_PostEffectRenderSetup, and m_BackBufferRenderSetup still \
             point at it, so the desktop view stays dead until the engine next resizes on its own",
        );
        return;
    }
    let Some(backing) = BACKING.lock().take() else {
        return;
    };
    let device = engine.m_Device.cast::<HDevice_t>();
    if device.is_null() {
        // Nothing can be freed without the device. Better to leak the texture than to call into a
        // null device during teardown; the process is going away regardless.
        tracing::warn!(target: "vr", "back buffer: no device at release; leaking the backing texture");
        return;
    }
    // SAFETY: ownership is clear and the engine has been resized off this texture, so nothing
    // references it. `BACKING.take()` above makes a second release impossible.
    unsafe { Destroy2DTexture(device, backing.texture) };
    tracing::info!(
        target: "vr",
        width = backing.size.0,
        height = backing.size.1,
        "back buffer: released the mod-owned backing texture",
    );
}

/// The `CreateRenderSetups` epilogue: swap the engine's freshly built back-buffer objects for
/// mod-owned equivalents.
///
/// Called on the render thread with the draw thread drained (`CreateRenderSetups` only runs from the
/// `Draw` prologue), immediately after the engine has rebuilt its own three objects. Inert unless the
/// mod owns the back buffer.
///
/// # Safety
/// `engine` must be the live graphics engine with its render setups freshly constructed, on the
/// drained idle context that `CreateRenderSetups` guarantees.
pub unsafe fn substitute_render_setups(engine: &mut GraphicsEngine) {
    // Whatever was installed is gone: `DestroyRenderSetups` freed it and the engine has just rebuilt
    // its own objects over the swapchain. Clearing first means the flag always describes the engine's
    // present state, whichever branch runs below.
    let was_installed = INSTALLED.swap(false, Ordering::AcqRel);

    if !owned() {
        // Ownership was released and the engine is back on its own objects, so this is the moment the
        // backing texture stops being referenced -- and the only moment we can know that. Freeing it
        // here covers both a mid-session release and eject, without either having to reason about
        // when the rebuild happened.
        if was_installed {
            // SAFETY: `INSTALLED` is now false and the engine's fields hold its own freshly built
            // objects, so nothing references the texture.
            unsafe { release_backing_texture(engine) };
        }
        return;
    }

    // SAFETY: the caller guarantees a freshly built set of engine render setups on the drained idle
    // context.
    match unsafe { substitute(engine) } {
        Ok(()) => INSTALLED.store(true, Ordering::Release),
        Err(e) => {
            // Leave the engine's own objects in place and stand down rather than run on
            // half-installed state: a partial substitution is the one shape that could leave a field
            // pointing at something neither side owns. Clear the config flag too, not just the
            // runtime one -- otherwise the next frame sees the flag still set, takes ownership again,
            // and drives another full `ApplyResize`, turning a persistent failure into a per-frame
            // teardown-and-rebuild storm.
            OWNED.store(false, Ordering::Release);
            crate::config::CONFIG.lock().vr.own_back_buffer = false;
            tracing::error!(
                target: "vr",
                "back buffer: substitution failed; disabling vr.own_back_buffer: {e:#}",
            );
        }
    }
}

/// Build the mod-owned surface and render setups and install them over the engine's, freeing the
/// engine's originals afterwards.
///
/// # Safety
/// As [`substitute_render_setups`].
unsafe fn substitute(engine: &mut GraphicsEngine) -> anyhow::Result<()> {
    let device = engine.m_Device.cast::<HDevice_t>();
    anyhow::ensure!(!device.is_null(), "the graphics device is unavailable");
    // The size the engine has just built everything else at, so the substitute matches the scene
    // targets and the depth surface it will be paired with.
    let size = {
        let info = &unsafe { engine.m_Device.as_ref() }
            .context("the graphics device is unavailable")?
            .m_DeviceInfo;
        (info.m_DisplayWidth, info.m_DisplayHeight)
    };
    anyhow::ensure!(
        size.0 > 0 && size.1 > 0,
        "the device reports a zero display size"
    );

    let mut backing = BACKING.lock();
    if backing.as_ref().map(|b| b.size) != Some(size) {
        // Free the old texture before allocating the new one: the engine's `DestroyRenderSetups` has
        // already dropped the surface that referenced it, so nothing points at it any more.
        if let Some(old) = backing.take() {
            unsafe { Destroy2DTexture(device, old.texture) };
        }
        let texture = unsafe { create_backing_texture(device, size) }?;
        *backing = Some(Backing { texture, size });
        tracing::info!(
            target: "vr",
            width = size.0,
            height = size.1,
            "back buffer: created the mod-owned backing texture",
        );
    }
    let texture = backing
        .as_ref()
        .expect("the backing texture was just ensured")
        .texture;

    // A fresh surface per substitution, matching the engine's own construction: it goes into
    // `m_BackBufferLinear`, so the engine's next `DestroyRenderSetups` frees it.
    let surface = unsafe { GetRenderTarget(device, texture, 0, 0, 0, 1) };
    anyhow::ensure!(
        !surface.is_null(),
        "the render target over the backing texture was not created"
    );

    // Pair the composite setup with the engine's freshly rebuilt depth surface, which is already at
    // this same size -- that agreement is the whole point of substituting here rather than earlier.
    // A null depth would silently produce a colour-only composite where the engine's own setup has
    // one, so it is a failure rather than something to paper over.
    let depth = engine.m_MainDepthSurface.cast::<HTexture_t>();
    if depth.is_null() {
        // SAFETY: the surface was just created and is not yet reachable from the engine.
        unsafe { DestroySurface(device, surface) };
        anyhow::bail!("the engine's main depth surface is unavailable");
    }

    // Both setups are built before either is installed, so a failure of the second cannot strand the
    // first: nothing here is reachable from the engine until the writes below, which means anything
    // already created has to be freed on the way out.
    let post_effect = match unsafe { create_setup(device, surface, std::ptr::null_mut()) } {
        Ok(setup) => setup,
        Err(e) => {
            // SAFETY: as above -- the surface is still mod-private.
            unsafe { DestroySurface(device, surface) };
            return Err(e).context("creating the post-effect render setup");
        }
    };
    let composite = match unsafe { create_setup(device, surface, depth) } {
        Ok(setup) => setup,
        Err(e) => {
            // SAFETY: neither object is reachable from the engine; consumers before the thing they
            // consume, as below.
            unsafe {
                DestroyRenderSetup(device, post_effect);
                DestroySurface(device, surface);
            }
            return Err(e).context("creating the composite render setup");
        }
    };

    // Stash the engine's originals, install ours, then free the originals. Installing first keeps
    // every field non-null for the whole window, so nothing can observe a torn state.
    let old_linear = engine.m_BackBufferLinear;
    let old_post_effect = engine.m_PostEffectRenderSetup;
    let old_composite = engine.m_BackBufferRenderSetup;

    engine.m_BackBufferLinear = surface.cast::<Texture>();
    engine.m_PostEffectRenderSetup = post_effect;
    engine.m_BackBufferRenderSetup = composite;
    engine.m_RenderContext.m_RenderSetup = composite;

    // Consumers before the thing they consume: the setups reference the surface.
    unsafe {
        DestroyRenderSetup(device, old_post_effect);
        DestroyRenderSetup(device, old_composite);
        DestroySurface(device, old_linear.cast::<HTexture_t>());
    }
    Ok(())
}

/// Create the mod-owned backing texture: a plain single-mip render target in the format the engine's
/// own back buffer uses, so no alias is needed to bridge a format gap (`docs/mod/swapchain-ownership.md` §4.5).
///
/// # Safety
/// `device` must be the live graphics device.
unsafe fn create_backing_texture(
    device: *mut HDevice_t,
    size: (u32, u32),
) -> anyhow::Result<*mut HTexture_t> {
    // Written through a raw pointer into zeroed storage, never materialised as a Rust value: the
    // engine leaves several of these fields at zero, and zero is not a valid discriminant for every
    // enum among them (`TileMode` has no zero variant), so producing the struct by value would be
    // instant undefined behaviour -- as a `mem::zeroed()` first cut discovered. This way the bytes
    // are exactly what the engine's own call sites pass, and no enum validity is ever asserted.
    let mut params = std::mem::MaybeUninit::<Create2DTextureParams>::zeroed();
    let p = params.as_mut_ptr();
    // SAFETY: `p` points at properly aligned, zeroed storage for the params; every write below is a
    // field store through the pointer, so no reference to a partially valid struct is created.
    unsafe {
        (*p).m_Width = size.0;
        (*p).m_Height = size.1;
        (*p).m_NumSlices = 1;
        (*p).m_NumMipLevels = 1;
        (*p).m_Format = SurfaceFormat::ABGR32;
        (*p).m_MultisampleType = MultisampleFormat::None;
        (*p).m_UsageType = UsageType::RenderTarget;
        (*p).m_PoolType = PoolType::GfxMem;
        (*p).m_Name = BACKING_NAME.as_ptr();
    }

    let texture = unsafe { Create2DTexture(device, p.cast_const()) };
    anyhow::ensure!(!texture.is_null(), "the backing texture was not created");
    Ok(texture)
}

/// Create a render setup over `colour` (and optionally `depth`), with the same parameters the engine
/// uses for both of its back-buffer setups.
///
/// # Safety
/// `device`, `colour`, and any non-null `depth` must be live handles.
unsafe fn create_setup(
    device: *mut HDevice_t,
    colour: *mut HTexture_t,
    depth: *mut HTexture_t,
) -> anyhow::Result<*mut HRenderSetup_t> {
    // Written through a raw pointer into zeroed storage for the same reason as the texture params
    // above: zero is not a valid discriminant for every enum in this struct.
    let mut params = std::mem::MaybeUninit::<CreateRenderSetupParams>::zeroed();
    let p = params.as_mut_ptr();
    // SAFETY: as in `create_backing_texture`.
    unsafe {
        (*p).m_DepthTarget = depth;
        (*p).m_ColorTargets[0] = colour;
        (*p).m_MultisampleFormat = MultisampleFormat::None;
        (*p).m_Mask = 15;
        // `m_AutoResolve = 1`, `m_EDRAMLayout = 0`, `m_UAVStart = 15` (the "immediately after the
        // colour targets" sentinel) -- the values `CreateRenderSetups` uses for both back-buffer
        // setups.
        (*p).m_Flags = 0x79;
    }

    let setup = unsafe { CreateRenderSetup(device, p.cast_const()) };
    anyhow::ensure!(!setup.is_null(), "the render setup was not created");
    Ok(setup)
}
