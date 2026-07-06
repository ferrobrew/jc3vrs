//! The desktop mirror: while an OpenXR session runs, show one eye in the game's own window.
//!
//! ## Why a mirror is needed
//!
//! While a session runs the compositor owns the HMD present and the engine's own present is
//! suppressed for both eyes (`BLOCK_FLIP`, `docs/rendering.md` §7), so the game window would freeze
//! on a stale frame. This module presents the game's own swapchain itself -- exactly once per frame,
//! unsynced -- with one eye's capture drawn into the back buffer.
//!
//! ## The letterbox pre-compensation
//!
//! While the session runs the engine's swapchain buffers are resized to the per-eye render
//! resolution (`vr.native_resolution`, `docs/rendering.md` §9), which is near-square, but the Win32
//! window keeps its original (usually 16:9) client rect. DXGI's BitBlt present stretches the whole
//! back buffer onto the window client rect, so a straight full-buffer draw would be distorted. We
//! pre-compensate: [`letterbox_viewport`] computes a viewport *inside the buffer* such that, after the
//! buffer→window stretch, the eye image lands at its own aspect, centered, with black bars. A window
//! resize by the WM only changes the client rect, so the viewport recomputes next frame; the VR path
//! is never touched.
//!
//! ## Gamma
//!
//! Straight passthrough, no gamma conversion (unlike [`crate::vr::blit`], which linearizes for an
//! `_SRGB` swapchain). The captured eye texture is a `CopyResource` of `m_BackBufferLinear` as
//! `R8G8B8A8_UNORM` (non-sRGB) holding **display-referred** bytes -- the same bytes the game itself
//! presents to its non-sRGB desktop swapchain, which look correct. The game swapchain back buffer we
//! draw into is that same non-sRGB format, so an RTV over it applies **no** hardware encode: writing
//! the sampled bytes unchanged reproduces exactly what the game would have presented. So the mirror
//! reuses the capture composite's plain-sample pixel shader; no gamma is a genuine correctness
//! conclusion, not a guess, so it is not a config knob.
//!
//! ## Threading
//!
//! Everything runs on the game's main thread, after both eyes have drawn and the draw worker has
//! drained (`hooks::game`). The letterbox draw, egui composite, and present run under the engine's
//! `Context::m_Mutex`, serialized with the engine's render work, the same discipline as
//! [`crate::capture`] and [`crate::vr::blit`].

use anyhow::Context as _;
use parking_lot::Mutex;
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            Direct3D11::{
                D3D11_COMPARISON_NEVER, D3D11_CULL_NONE, D3D11_FILL_SOLID,
                D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_RASTERIZER_DESC, D3D11_SAMPLER_DESC,
                D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC, D3D11_VIEWPORT,
                ID3D11DeviceContext, ID3D11PixelShader, ID3D11RasterizerState,
                ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView,
                ID3D11Texture2D, ID3D11VertexShader,
            },
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
        UI::WindowsAndMessaging::GetClientRect,
    },
    core::Interface as _,
};

use jc3gi::graphics_engine::{device::Device, graphics_engine::get_graphics_params};

use crate::ui::render::EGUI_DEBUG_RENDER_STATE;

/// The committed, precompiled shaders shared with the capture composite: the fullscreen-triangle
/// vertex shader and the plain-sample (no gamma) pixel shader. See the module gamma note.
const VERTEX_DXBC: &[u8] = include_bytes!("../shaders/capture_vs.dxbc");
const PIXEL_DXBC: &[u8] = include_bytes!("../shaders/capture_ps.dxbc");

/// The mirror pipeline singleton, built lazily on the first mirrored frame and torn down with the
/// runtime. Holds COM objects, which `windows` marks `Send`/`Sync`, so a `Mutex` static is sound.
static MIRROR: Mutex<Option<Mirror>> = Mutex::new(None);

/// Draw the configured eye's capture into the game swapchain's back buffer (letterboxed to the window
/// aspect), composite the egui overlay on top, and present the game swapchain unsynced.
///
/// Called once per frame from the stereo Draw driver, after the eyes have drawn and drained and the
/// XR frame has been submitted, only while a session is running and `vr.mirror` is on. Any failure
/// logs on target `"vr"`, disables `vr.mirror` at runtime (the game window then holds its last frame),
/// and never crashes or wedges the frame loop.
///
/// **Unsynced by mandate:** the present uses `SyncInterval = 0`. A vsynced mirror on a 60 Hz monitor
/// would block the game thread until the monitor's next scanout and throttle the whole frame loop --
/// including the 90 Hz HMD submit -- down to the monitor's refresh. The compositor, not this present,
/// paces the HMD.
pub fn present_mirror(eye: usize) {
    if let Err(e) = unsafe { present_mirror_inner(eye) } {
        tracing::error!(target: "vr", "mirror present failed; disabling vr.mirror: {e:#}");
        crate::config::CONFIG.lock().vr.mirror = false;
        *MIRROR.lock() = None;
    }
}

/// Tear down the mirror pipeline (COM release). Called from the runtime cleanup.
pub fn teardown() {
    *MIRROR.lock() = None;
}

/// # Safety
/// Reads engine singletons via raw pointer dereference; the graphics engine, device, and context must
/// be live (they are while the hooks are installed and a frame is in flight on the game thread).
unsafe fn present_mirror_inner(eye: usize) -> anyhow::Result<()> {
    let eye = eye.min(1);

    // Clone (AddRef) the captured eye texture under a brief EGUI lock, dropped before the mirror
    // state / context work, mirroring the blit's lock discipline.
    let eye_texture: Option<ID3D11Texture2D> = {
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        lock.texture(eye).cloned()
    };
    let eye_texture = eye_texture.context("vr: the mirror eye capture is unavailable")?;

    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable for the mirror")?;
    let device = unsafe { ge.m_Device.as_ref() }
        .context("vr: the graphics device is unavailable for the mirror")?;
    let context = unsafe { device.m_Context.as_ref() }
        .context("vr: the graphics context is unavailable for the mirror")?;
    let back_buffer = unsafe { device.m_BackBuffer.as_ref() }
        .context("vr: the game back buffer is unavailable for the mirror")?;

    let buffer_size = (
        u32::from(back_buffer.m_Width),
        u32::from(back_buffer.m_Height),
    );
    let (src_w, src_h) = unsafe { texture_size(&eye_texture) };
    let window_size = client_size().context("vr: the game window client rect is unavailable")?;
    let viewport = letterbox_viewport(
        AspectSize {
            width: src_w,
            height: src_h,
        },
        buffer_size,
        window_size,
    );

    // The engine wraps swapchain back buffer 0 (`docs/rendering.md` §7); drawing into its resource and
    // then presenting the swapchain is the engine's own present path, minus the vsync/bookkeeping.
    let back_texture = &back_buffer.m_Texture;

    let mut guard = MIRROR.lock();
    if guard.is_none() {
        *guard = Some(Mirror::new(device)?);
    }
    let mirror = guard
        .as_mut()
        .expect("the mirror pipeline was just ensured");

    let rtv = mirror.rtv_for(device, back_texture)?;
    let srv = mirror.srv_for(device, &eye_texture)?;

    // The letterbox draw, egui composite, and present run under one critical-section hold, serialized
    // with the engine's render work. The section is recursive, so a re-entrant take inside egui (there
    // is none today) would be safe.
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        mirror.draw(&context.m_Context, &rtv, &srv, viewport);

        // Composite the egui debug overlay onto the same back buffer so it reaches the desktop while a
        // session runs -- the overlay normally rides the engine flip path, which `BLOCK_FLIP`
        // suppresses in VR, so without this it would be invisible on the desktop. Gated exactly as
        // `graphics_flip` gates it: hidden while the F10 capture window is up so recordings stay clean.
        // The overlay binds the back buffer's own RTV and lays out in window-client pixels, so on the
        // near-square per-eye buffer it renders at the buffer's scale and is stretched with everything
        // else -- acceptable for a debug overlay; the in-headset UI is the floating panel, not egui.
        if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut()
            && !crate::capture::is_active()
        {
            egui_state.render();
        }

        // Present the game swapchain ourselves -- the only present this frame (the engine's is
        // blocked). Unsynced by mandate (see the doc comment); errors bubble up to disable the mirror.
        let present = device
            .m_SwapChain
            .Present(0, windows::Win32::Graphics::Dxgi::DXGI_PRESENT::default());
        LeaveCriticalSection(context.m_Mutex);
        present
            .ok()
            .context("vr: the mirror swapchain present failed")?;
    }
    Ok(())
}

/// A width/height pair whose aspect (`width / height`) is preserved by the letterbox fit.
#[derive(Copy, Clone)]
struct AspectSize {
    width: u32,
    height: u32,
}

/// A viewport rectangle within the swapchain back buffer, in buffer pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Viewport {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Compute the back-buffer viewport that, after DXGI stretches the whole buffer onto the window
/// client rect, places the source image at its own aspect, centered, letterboxed.
///
/// The mapping is: a fullscreen triangle fills the returned viewport with the source image, so in
/// *window* space the image must occupy the aspect-fit rect of `source` inside `window` (centered,
/// with bars). DXGI stretches buffer→window linearly and independently per axis, so a window rect
/// `(wx, wy, tw, th)` is produced by the buffer rect `(wx·bw/ww, wy·bh/wh, tw·bw/ww, th·bh/wh)`.
/// Substituting the aspect-fit rect gives the result. A degenerate size returns the full buffer
/// (a straight, possibly-stretched draw -- the safe fallback).
fn letterbox_viewport(source: AspectSize, buffer: (u32, u32), window: (u32, u32)) -> Viewport {
    let (bw, bh) = (buffer.0 as f32, buffer.1 as f32);
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let full = Viewport {
        x: 0.0,
        y: 0.0,
        width: bw,
        height: bh,
    };
    if source.width == 0
        || source.height == 0
        || buffer.0 == 0
        || buffer.1 == 0
        || window.0 == 0
        || window.1 == 0
    {
        return full;
    }

    let image_aspect = source.width as f32 / source.height as f32;
    let window_aspect = ww / wh;

    // Aspect-fit the image into the window client rect (in window pixels).
    let (target_w, target_h) = if image_aspect > window_aspect {
        // Image is wider than the window: fit to width, bars top and bottom.
        (ww, ww / image_aspect)
    } else {
        // Image is taller (or equal): fit to height, bars left and right.
        (wh * image_aspect, wh)
    };
    let target_x = (ww - target_w) * 0.5;
    let target_y = (wh - target_h) * 0.5;

    // Pre-compensate for the per-axis buffer→window stretch.
    let sx = bw / ww;
    let sy = bh / wh;
    Viewport {
        x: target_x * sx,
        y: target_y * sy,
        width: target_w * sx,
        height: target_h * sy,
    }
}

/// The game window's client size (`width`, `height`) in pixels, or `None` if it cannot be read. The
/// game HWND is reached the same way [`crate::vr::resolution`] does, via the graphics params.
fn client_size() -> Option<(u32, u32)> {
    let hwnd: HWND = unsafe { get_graphics_params() }.m_Hwnd;
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the live game window handle; `GetClientRect` errors on a bad handle.
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// The pixel dimensions of a texture from its descriptor.
///
/// # Safety
/// `texture` must be a live `ID3D11Texture2D`.
unsafe fn texture_size(texture: &ID3D11Texture2D) -> (u32, u32) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
    }
    (desc.Width, desc.Height)
}

/// The mirror pipeline: shaders, sampler, rasterizer, and caches of the back-buffer render-target view
/// and the eye-capture shader-resource view, both keyed on the underlying resource pointer so an
/// engine resize (which changes the back-buffer / capture identity) rebuilds the view.
struct Mirror {
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    sampler: ID3D11SamplerState,
    rasterizer: ID3D11RasterizerState,
    /// The RTV over the game back buffer, keyed on the back-buffer texture pointer.
    rtv_cache: Option<(usize, ID3D11RenderTargetView)>,
    /// The SRV over the captured eye texture, keyed on the capture texture pointer.
    srv_cache: Option<(usize, ID3D11ShaderResourceView)>,
}

impl Mirror {
    fn new(device: &Device) -> anyhow::Result<Self> {
        let d3d = &device.m_Device;
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut vertex_shader: Option<ID3D11VertexShader> = None;
            d3d.CreateVertexShader(VERTEX_DXBC, None, Some(&mut vertex_shader))
                .context("vr: creating the mirror vertex shader")?;
            let vertex_shader =
                vertex_shader.context("vr: the mirror vertex shader was not created")?;

            let mut pixel_shader: Option<ID3D11PixelShader> = None;
            d3d.CreatePixelShader(PIXEL_DXBC, None, Some(&mut pixel_shader))
                .context("vr: creating the mirror pixel shader")?;
            let pixel_shader =
                pixel_shader.context("vr: the mirror pixel shader was not created")?;

            let mut sampler: Option<ID3D11SamplerState> = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MinLOD: f32::MIN,
                    MaxLOD: f32::MAX,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 0,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                },
                Some(&mut sampler),
            )
            .context("vr: creating the mirror sampler")?;
            let sampler = sampler.context("vr: the mirror sampler was not created")?;

            let mut rasterizer: Option<ID3D11RasterizerState> = None;
            d3d.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )
            .context("vr: creating the mirror rasterizer state")?;
            let rasterizer =
                rasterizer.context("vr: the mirror rasterizer state was not created")?;

            Ok(Self {
                vertex_shader,
                pixel_shader,
                sampler,
                rasterizer,
                rtv_cache: None,
                srv_cache: None,
            })
        }
    }

    /// Clear the whole back buffer to black (the letterbox bars), then draw the eye into `viewport`.
    /// The caller must hold the context mutex.
    ///
    /// # Safety
    /// `context` must be the live engine immediate context; `rtv`/`srv` must be live views over the
    /// game back buffer and the captured eye texture respectively.
    unsafe fn draw(
        &self,
        context: &ID3D11DeviceContext,
        rtv: &ID3D11RenderTargetView,
        srv: &ID3D11ShaderResourceView,
        viewport: Viewport,
    ) {
        unsafe {
            context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            // Clear the full target to opaque black first: this paints the letterbox bars outside the
            // eye viewport.
            context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);

            context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: viewport.x,
                TopLeftY: viewport.y,
                Width: viewport.width,
                Height: viewport.height,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            context.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            context.RSSetState(&self.rasterizer);
            context.Draw(3, 0);

            // Unbind our SRV so the egui overlay / next frame does not see it still bound. Leave the
            // RTV bound: egui's own `render` binds the back-buffer RTV itself, and the present reads
            // the buffer regardless.
            context.PSSetShaderResources(0, Some(&[None]));
        }
    }

    /// Get (creating and caching on first use, rebuilding on a back-buffer change) the RTV over the
    /// game back buffer. Device methods are free-threaded.
    fn rtv_for(
        &mut self,
        device: &Device,
        back_texture: &jc3gi::graphics_engine::texture::ID3D11Resource,
    ) -> anyhow::Result<ID3D11RenderTargetView> {
        let ptr = back_texture.as_raw() as usize;
        if let Some((cached, rtv)) = self.rtv_cache.as_ref()
            && *cached == ptr
        {
            return Ok(rtv.clone());
        }
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe {
            device
                .m_Device
                .CreateRenderTargetView(back_texture, None, Some(&mut rtv))
        }
        .context("vr: creating the game back-buffer render-target view")?;
        let rtv = rtv.context("vr: the game back-buffer render-target view was not created")?;
        self.rtv_cache = Some((ptr, rtv.clone()));
        Ok(rtv)
    }

    /// Get (creating and caching on first use, rebuilding on a texture change) the SRV over the
    /// captured eye texture.
    fn srv_for(
        &mut self,
        device: &Device,
        src: &ID3D11Texture2D,
    ) -> anyhow::Result<ID3D11ShaderResourceView> {
        let ptr = src.as_raw() as usize;
        if let Some((cached, srv)) = self.srv_cache.as_ref()
            && *cached == ptr
        {
            return Ok(srv.clone());
        }
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        unsafe {
            device
                .m_Device
                .CreateShaderResourceView(src, None, Some(&mut srv))
        }
        .context("vr: creating the mirror eye shader-resource view")?;
        let srv = srv.context("vr: the mirror eye shader-resource view was not created")?;
        self.srv_cache = Some((ptr, srv.clone()));
        Ok(srv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(x: f32, y: f32, width: f32, height: f32) -> Viewport {
        Viewport {
            x,
            y,
            width,
            height,
        }
    }

    fn approx(a: Viewport, b: Viewport) {
        let eps = 1e-3;
        assert!(
            (a.x - b.x).abs() < eps
                && (a.y - b.y).abs() < eps
                && (a.width - b.width).abs() < eps
                && (a.height - b.height).abs() < eps,
            "viewport mismatch: {a:?} != {b:?}",
        );
    }

    /// The core check: after the buffer→window stretch, the viewport must map back to the aspect-fit
    /// window rect. Re-derives the window rect from the viewport and compares against an independent
    /// aspect-fit, so the test does not just restate the formula.
    fn assert_letterboxes(source: AspectSize, buffer: (u32, u32), window: (u32, u32)) {
        let v = letterbox_viewport(source, buffer, window);
        let (bw, bh) = (buffer.0 as f32, buffer.1 as f32);
        let (ww, wh) = (window.0 as f32, window.1 as f32);
        // Stretch the viewport back into window space.
        let win_x = v.x * ww / bw;
        let win_y = v.y * wh / bh;
        let win_w = v.width * ww / bw;
        let win_h = v.height * wh / bh;

        // Independent aspect-fit in window space.
        let a = source.width as f32 / source.height as f32;
        let (want_w, want_h) = if a > ww / wh {
            (ww, ww / a)
        } else {
            (wh * a, wh)
        };
        let want_x = (ww - want_w) * 0.5;
        let want_y = (wh - want_h) * 0.5;

        let eps = 1e-2;
        assert!((win_x - want_x).abs() < eps, "x: {win_x} != {want_x}");
        assert!((win_y - want_y).abs() < eps, "y: {win_y} != {want_y}");
        assert!((win_w - want_w).abs() < eps, "w: {win_w} != {want_w}");
        assert!((win_h - want_h).abs() < eps, "h: {win_h} != {want_h}");
        // The presented image keeps the source aspect.
        assert!(
            (win_w / win_h - a).abs() < eps,
            "presented aspect {} != source {a}",
            win_w / win_h,
        );
        // The viewport must stay inside the buffer.
        assert!(
            v.x >= -eps && v.y >= -eps,
            "viewport origin outside buffer: {v:?}"
        );
        assert!(
            v.x + v.width <= bw + eps && v.y + v.height <= bh + eps,
            "viewport exceeds buffer: {v:?} in {bw}x{bh}",
        );
    }

    #[test]
    fn square_eye_into_widescreen_window_letterboxes_sideways() {
        // Near-square per-eye buffer, 16:9 window: bars left and right.
        let v = letterbox_viewport(
            AspectSize {
                width: 1600,
                height: 1600,
            },
            (1600, 1600),
            (1920, 1080),
        );
        // Fit to height: window rect is 1080x1080 centered, x=(1920-1080)/2=420. In buffer: the full
        // height (1600) and width 1080*1600/1920=900 at x=420*1600/1920=350.
        approx(v, vp(350.0, 0.0, 900.0, 1600.0));
    }

    #[test]
    fn matching_aspect_fills_the_buffer() {
        // When the source, buffer, and window all share an aspect there are no bars and the viewport
        // is the whole buffer (a straight full-buffer draw).
        let v = letterbox_viewport(
            AspectSize {
                width: 1920,
                height: 1080,
            },
            (1920, 1080),
            (1920, 1080),
        );
        approx(v, vp(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn source_wider_than_window_bars_top_and_bottom() {
        // An ultra-wide source in a 4:3 window: fit to width, bars top and bottom.
        assert_letterboxes(
            AspectSize {
                width: 3440,
                height: 1440,
            },
            (2048, 2048),
            (1024, 768),
        );
    }

    #[test]
    fn stretched_buffer_recovers_correct_aspect() {
        // The buffer aspect differs from both the source and the window; the compensation must still
        // land the source at its own aspect on the window.
        assert_letterboxes(
            AspectSize {
                width: 1832,
                height: 1920,
            },
            (1832, 1920),
            (2560, 1440),
        );
        assert_letterboxes(
            AspectSize {
                width: 1440,
                height: 1600,
            },
            (1440, 1600),
            (1600, 900),
        );
    }

    #[test]
    fn degenerate_sizes_fall_back_to_full_buffer() {
        let full = vp(0.0, 0.0, 1280.0, 720.0);
        approx(
            letterbox_viewport(
                AspectSize {
                    width: 0,
                    height: 0,
                },
                (1280, 720),
                (1920, 1080),
            ),
            full,
        );
        approx(
            letterbox_viewport(
                AspectSize {
                    width: 100,
                    height: 100,
                },
                (1280, 720),
                (0, 1080),
            ),
            full,
        );
    }
}
