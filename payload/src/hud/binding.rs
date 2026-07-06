//! The game-side UI rebinding operations: pointing the Scaleform UI at our target, and restoring the
//! engine's own binding. Both touch the live UI singleton, so they must run on the render thread.

use jc3gi::ui::ui_manager::GetIUIManager;
use windows::core::Interface as _;

use super::target::HudTarget;

/// Rebind the UI to render into `target` at `width` x `height` pixels. Returns whether it took: the
/// UI singleton must be live.
///
/// The engine's own `RestoreAfterReset` sizes everything from the device resolution via
/// `ComputeMovieSizeOnViewSize`, which forces the game-window aspect and a centering offset that
/// scales with the target size. We bypass it and drive the render rectangle directly, so the HUD
/// fills our texture at its own aspect:
///
/// 1. `m_MovieScaleWidth`/`m_MovieScaleHeight` -- the movie render rectangle. Set to our texture
///    dimensions so the movie fills it. Set directly (not via `ComputeMovieSizeOnViewSize`, which
///    would overwrite them from the device resolution).
/// 2. `m_CachedViewportWidth`/`m_CachedViewportHeight` -- the safe-area input. The UI anchors
///    elements to the safe area, which `ComputeSafeArea` expands to the viewport's aspect, so we
///    point it at our texture and recompute the safe area (and game-view area) to reflow the UI to
///    our aspect rather than the window's.
/// 3. `SetMovieViewport(width, height)` -- the Scaleform HAL's actual viewport. With the viewport
///    equal to the movie rectangle, the centering offset is zero and the movie fills the target.
/// 4. `InitPlatformRT` + `UpdateData` -- rebuilds the `RenderBuffer` and swaps its RTV/DSV to our
///    texture.
///
/// The cached viewport size is refreshed from the device every frame (in `PreRender`), but the safe
/// area, movie rectangle, and movie viewport are not recomputed per frame, so this rebind persists
/// until the next device/resolution reset (handled by re-applying on a back-buffer size change).
pub(super) fn redirect_to(target: &HudTarget, width: u32, height: u32) -> bool {
    // SAFETY: GetIUIManager returns the live UI singleton. The sequence below touches only UIManager
    // fields and calls only UIManager methods, all of which the engine itself drives from this same
    // render thread.
    unsafe {
        let Some(manager) = GetIUIManager().as_mut() else {
            tracing::warn!("hud redirect: UIManager not available");
            return false;
        };

        let w = width as i32;
        let h = height as i32;
        tracing::debug!(target: "hud", width, height, "applying redirect");

        // 1. Movie render rectangle = our texture, so the movie fills it.
        manager.m_MovieScaleWidth = w;
        manager.m_MovieScaleHeight = h;

        // 2. Reflow the UI safe area to our aspect instead of the window's.
        manager.m_CachedViewportWidth = w;
        manager.m_CachedViewportHeight = h;
        manager.ComputeSafeArea();
        manager.ComputeGameViewArea();

        // 3. Viewport = movie rectangle, so the centering offset is zero.
        manager.SetMovieViewport(w, h);

        // 4. Rebuild the RenderBuffer and swap its views to our texture. InitPlatformRT builds the
        //    render target square (side = width), so patch its height for a non-square texture --
        //    otherwise the HAL's viewport/scissor would not match the actual target. At aspect 1.0
        //    this is a no-op (width == height).
        manager.InitPlatformRT(w);
        let Some(render_buffer) = manager.m_RenderBuffer.as_mut() else {
            tracing::warn!("hud redirect: m_RenderBuffer null after InitPlatformRT");
            return false;
        };
        render_buffer.m_BufferHeight = h;
        render_buffer.m_ViewRectBottom = h;
        render_buffer.UpdateData(
            target.color_rtv().as_raw(),
            std::ptr::null_mut(),
            target.depth_dsv().as_raw(),
        );

        tracing::debug!(target: "hud", width, height, "redirect applied");
    }
    true
}

/// Whether the movie's live viewport still matches the redirect: `SetMovieViewport` fills the
/// movie's `GFx::Viewport` with buffer = viewport = movie rectangle = our texture (zero centering
/// offset), and the movie's `ViewportMatrix` -- the render transform AND the entire mouse-to-stage
/// transform -- is derived from it. The engine's device-reset path (`RestoreAfterReset`) is the
/// one writer that can silently replace it (resizing the movie from the device resolution), which
/// skews the mouse hit test off the texture-shaped render. Returns `true` while the movie is not
/// live yet (nothing to drift).
pub(super) fn movie_viewport_matches(width: u32, height: u32) -> bool {
    // SAFETY: reads plain fields of the live UI singleton and its movie on the render thread,
    // where the engine itself reads them.
    unsafe {
        let Some(manager) = GetIUIManager().as_mut() else {
            return true;
        };
        let Some(movie) = manager.m_Movie.as_ref() else {
            return true;
        };
        let viewport = &movie.Viewport;
        (
            viewport.BufferWidth,
            viewport.BufferHeight,
            viewport.Left,
            viewport.Top,
            viewport.Width,
            viewport.Height,
        ) == (
            width as i32,
            height as i32,
            0,
            0,
            width as i32,
            height as i32,
        )
    }
}

/// Restore the engine's own UI binding by resizing everything back to the back buffer:
/// `ComputeMovieSizeOnViewSize` resets the movie rectangle to the device resolution, then the
/// viewport, safe area, and `InitPlatformRT` (which rebinds to the engine surface) follow.
pub(super) fn restore_engine_binding(back_buffer_width: u32, back_buffer_height: u32) {
    // SAFETY: same as redirect_to -- the engine drives this sequence from the render thread.
    unsafe {
        let Some(manager) = GetIUIManager().as_mut() else {
            return;
        };

        let w = back_buffer_width as i32;
        let h = back_buffer_height as i32;
        tracing::debug!(target: "hud", back_buffer_width, back_buffer_height, "restoring engine binding");

        // Reset the movie rectangle to the device resolution (this refreshes the cached sizes too),
        // then replay the engine-native viewport / safe area / render-target rebind.
        manager.ComputeMovieSizeOnViewSize(true, false);
        manager.SetMovieViewport(w, h);
        manager.ComputeSafeArea();
        manager.ComputeGameViewArea();
        manager.InitPlatformRT(w);
    }
}
