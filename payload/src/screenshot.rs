//! On-demand back-buffer screenshots to the session's `screenshots/` folder (F12), for in-headset
//! diagnosis when the F10 stereo capture is inconvenient (it toggles a fullscreen mode). Captures the
//! linear back buffer -- under single-pass collapse that is the full side-by-side render, both
//! eye-halves as the GPU produced them, so a single PNG shows both eyes at once.
//!
//! The capture is split across two threads because the render-thread half runs inside the engine's
//! immediate-context critical section, with the draw thread and the whole frame blocked behind it.
//! Only the work that genuinely needs the immediate context stays there -- the staging copy, the map,
//! and a memcpy of the rows out of the mapping -- and the pixel conversion, the PNG encode, and the
//! file writes are handed to a short-lived writer thread. Inline, a double-wide capture is tens of
//! millions of pixels of scalar conversion plus a synchronous encode: seconds of frozen engine and a
//! dropped compositor frame on every press.
//!
//! A writer thread outliving the payload is the hazard that buys: a thread still executing when
//! [`crate::module::exit`] unmaps the image parks forever holding whatever it took. [`shutdown`]
//! closes that the same way [`crate::vr::tail::shutdown`] and the profiler's capture writer do.
//!
//! F12 capture is best-effort: if the writer thread cannot be spawned (resource exhaustion), the
//! capture is logged and dropped with no retry.

use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use anyhow::{Context as _, bail};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use windows::{
    Win32::Graphics::{
        Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
            ID3D11Texture2D,
        },
        Dxgi::Common::{
            DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
            DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        },
    },
    core::Interface as _,
};

use crate::stereo::single_pass::FrameDiagnostics;

/// Request a screenshot on the next rendered frame. Called from the F12 wndproc handler; the actual
/// capture runs on the render thread in [`capture_if_requested`].
pub fn request() {
    REQUESTED.store(true, Ordering::Relaxed);
}

/// If a screenshot was requested, copy `source` to a CPU-readable staging texture and read its pixels
/// back, then hand them to a writer thread that encodes and writes the PNG and its sidecar. Render
/// thread, inside the engine's context critical section; a no-op unless requested. Errors
/// (unsupported format, map failure, I/O) are logged, never propagated -- a screenshot must not take
/// the frame down with it.
pub fn capture_if_requested(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Resource,
) {
    if !REQUESTED.swap(false, Ordering::Relaxed) {
        return;
    }
    // Refuse once eject has begun: the writer this would spawn is exactly the thread [`shutdown`]
    // has to wait for, and by the time a request is serviced during teardown that wait may already
    // have passed.
    if crate::is_shutting_down() {
        return;
    }
    match read_back(device, context, source) {
        Ok(pending) => spawn_writer(pending),
        Err(e) => tracing::error!("screenshot: {e:#}"),
    }
}

/// Waits for any in-flight screenshot writers to finish, up to a bounded budget. Called on eject
/// alongside [`crate::vr::tail::shutdown`] and `profiler::capture::shutdown`, which this
/// mirrors rather than shares (the profiler module only exists under its feature, and a screenshot
/// must be writable without it): the writers are unjoined threads (spawned because encoding inline would freeze the frame
/// that spawned them for seconds), and a thread still executing when [`crate::module::exit`] unmaps
/// the image parks forever holding whatever it took, wedging the process with nothing left to log.
/// Waiting longer is strictly better than unloading underneath a live thread.
///
/// Returns whether the writers are confirmed stopped (or none were running). A `false` return means
/// the caller must not unload -- see [`crate::vr::tail::shutdown`]'s doc comment for the full
/// argument.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub fn shutdown() -> bool {
    for _ in 0..SHUTDOWN_POLLS {
        if PENDING_WRITES.load(Ordering::Acquire) == 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    tracing::error!(
        "screenshot: a writer did not finish within {} s; leaving the payload mapped rather than \
         unmapping under a live thread",
        SHUTDOWN_POLLS / 100,
    );
    false
}

static REQUESTED: AtomicBool = AtomicBool::new(false);
static COUNTER: AtomicU32 = AtomicU32::new(0);
/// The stamp for this run's `screenshots/<stamp>/` subfolder, taken on the first capture so every
/// screenshot of the run shares one folder.
static SCREENSHOT_STAMP: OnceLock<String> = OnceLock::new();

/// One read-back frame waiting to be encoded: everything the writer needs, owned, so nothing it
/// touches is engine state.
struct PendingWrite {
    /// The mapped rows, tightly packed at `width * 4` bytes, in the back buffer's own channel order
    /// (see `bgra`) and with its own alpha; [`to_rgba8_opaque`] normalizes both on the writer.
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    /// Whether the channel order is BGRA rather than RGBA.
    bgra: bool,
    /// The back-buffer format, for the sidecar (already formatted: `DXGI_FORMAT` is not `Send`-safe
    /// to reason about across the hand-off and the sidecar only ever prints it).
    format: String,
    /// The file name within this run's screenshot folder; numbered on the render thread so the
    /// sequence follows the order the frames were captured in, not the order the writers finish in.
    file_name: String,
    /// The single-pass state of the captured frame, snapshotted here because by the time the writer
    /// runs the engine is several frames on.
    diagnostics: Option<FrameDiagnostics>,
}

/// The render-thread half: staging copy, map, memcpy out. Everything here needs the immediate
/// context, and nothing that does not is allowed in.
fn read_back(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Resource,
) -> anyhow::Result<PendingWrite> {
    let texture: ID3D11Texture2D = source
        .cast()
        .context("the back buffer is not a Texture2D")?;

    // SAFETY: called on the render thread with the live immediate device/context; `texture` is the
    // live back-buffer resource. The staging copy + map read only this frame's finished back buffer.
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);

        let bgra = matches!(
            desc.Format,
            DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
        );
        let rgba = matches!(
            desc.Format,
            DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        );
        if !bgra && !rgba {
            bail!(
                "unsupported back-buffer format {:?} (only 8-bit RGBA/BGRA handled)",
                desc.Format
            );
        }

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
            .context("creating the staging texture")?;
        let staging = staging.context("the staging texture was not created")?;

        context.CopyResource(&staging, &texture);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("mapping the staging texture")?;

        let (width, height) = (desc.Width as usize, desc.Height as usize);
        let row_pitch = mapped.RowPitch as usize;
        let row_bytes = width * 4;
        let src = mapped.pData as *const u8;
        let mut pixels = vec![0u8; row_bytes * height];
        for y in 0..height {
            std::ptr::copy_nonoverlapping(
                src.add(y * row_pitch),
                pixels.as_mut_ptr().add(y * row_bytes),
                row_bytes,
            );
        }
        context.Unmap(&staging, 0);

        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(PendingWrite {
            pixels,
            width: desc.Width,
            height: desc.Height,
            bgra,
            format: format!("{:?}", desc.Format),
            file_name: format!("jc3vrs-{n:04}.png"),
            diagnostics: crate::stereo::single_pass::last_frame_diagnostics(),
        })
    }
}

/// Hands `pending` to a writer thread, counting it into [`PENDING_WRITES`] first so [`shutdown`]
/// cannot observe a gap between the decision to spawn and the thread starting.
fn spawn_writer(pending: PendingWrite) {
    // `Release` so that `shutdown`'s `Acquire` poll, once it observes the count fall back to zero,
    // also sees every write the thread performed before decrementing it.
    PENDING_WRITES.fetch_add(1, Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("jc3vrs-screenshot".to_owned())
        .spawn(move || {
            match write_screenshot(pending) {
                Ok(path) => tracing::info!("screenshot: wrote {}", path.display()),
                Err(e) => tracing::error!("screenshot: {e:#}"),
            }
            PENDING_WRITES.fetch_sub(1, Ordering::Release);
        });
    if let Err(e) = spawned {
        PENDING_WRITES.fetch_sub(1, Ordering::Release);
        tracing::error!("screenshot: could not spawn the writer thread: {e}");
    }
}

/// How many screenshot writers are still running. A counter rather than a flag: F12 can be pressed
/// again while the previous encode is still going, and two live writers must both be waited for.
static PENDING_WRITES: AtomicU32 = AtomicU32::new(0);

/// How long [`shutdown`] waits for the writers, in 10 ms polls. 10 s, matching the profiler's capture
/// writer: a double-wide capture is tens of millions of pixels to convert and deflate, and the cost
/// of waiting too long is only a slower eject, while unloading too early is the unrecoverable wedge
/// this exists to avoid.
const SHUTDOWN_POLLS: u32 = 1000;

/// The writer-thread half: convert, encode, write. Touches nothing the engine owns.
fn write_screenshot(mut pending: PendingWrite) -> anyhow::Result<PathBuf> {
    to_rgba8_opaque(&mut pending.pixels, pending.bgra);

    // All of a run's screenshots share one stamped subfolder (`screenshots/<stamp>/`), mirroring
    // the render-trace layout; the stamp is taken once, on the first capture.
    let stamp = SCREENSHOT_STAMP.get_or_init(crate::session::stamp);
    let dir = crate::session::subdir("screenshots")
        .map(|base| base.join(stamp))
        .context("could not resolve the session screenshots directory")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(&pending.file_name);

    let file =
        std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
    PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(
            &pending.pixels,
            pending.width,
            pending.height,
            ExtendedColorType::Rgba8,
        )
        .with_context(|| format!("encoding {}", path.display()))?;

    // Sidecar JSON: everything relevant about this frame's single-pass state (the per-eye cb13
    // matrices, the centre transform, the viewport, the config), so the exact matrices that
    // produced the image can be inspected offline instead of read from a log line.
    let sidecar = serde_json::json!({
        "image": path.file_name().and_then(|f| f.to_str()),
        "width": pending.width,
        "height": pending.height,
        "format": pending.format,
        "single_pass": serde_json::to_value(&pending.diagnostics)
            .unwrap_or(serde_json::Value::Null),
    });
    let json_path = path.with_extension("json");
    if let Err(e) = std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&sidecar).unwrap_or_default(),
    ) {
        tracing::warn!("screenshot: failed to write {}: {e}", json_path.display());
    }

    Ok(path)
}

/// Normalizes a mapped back-buffer row buffer in place: swaps R/B for the BGRA formats, and forces
/// alpha opaque.
///
/// The alpha is discarded rather than passed through because nothing downstream of the back buffer
/// ever reads it -- the VR blit and the desktop mirror both draw it into an opaque target with
/// blending off -- so the engine is free to leave it at whatever the last pass wrote, and it does:
/// the eye is cleared fully transparent behind a static-background full-screen UI. A PNG *is*
/// composited by its alpha, so passing that through turns a perfectly good capture into a viewer's
/// blank page.
fn to_rgba8_opaque(pixels: &mut [u8], bgra: bool) {
    for pixel in pixels.chunks_exact_mut(4) {
        if bgra {
            pixel.swap(0, 2);
        }
        pixel[3] = 0xFF;
    }
}
