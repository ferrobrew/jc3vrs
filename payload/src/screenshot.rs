//! On-demand back-buffer screenshots to the session's `screenshots/` folder (F12), for in-headset
//! diagnosis when the F10 stereo capture is inconvenient (it toggles a fullscreen mode). Captures the
//! linear back buffer -- under single-pass collapse that is the full side-by-side render, both
//! eye-halves as the GPU produced them, so a single PNG shows both eyes at once.

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

/// Request a screenshot on the next rendered frame. Called from the F12 wndproc handler; the actual
/// capture runs on the render thread in [`capture_if_requested`].
pub fn request() {
    REQUESTED.store(true, Ordering::Relaxed);
}

/// If a screenshot was requested, copy `source` to a CPU-readable staging texture, convert it to
/// RGBA8, and write it as a PNG in `<dll dir>/screenshots/`. Render thread; a no-op unless requested.
/// Errors (unsupported format, map failure, I/O) are logged, never propagated -- a screenshot must
/// never disturb the frame.
pub fn capture_if_requested(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Resource,
) {
    if !REQUESTED.swap(false, Ordering::Relaxed) {
        return;
    }
    match capture(device, context, source) {
        Ok(path) => tracing::info!("screenshot: wrote {}", path.display()),
        Err(e) => tracing::error!("screenshot: {e:#}"),
    }
}

static REQUESTED: AtomicBool = AtomicBool::new(false);
static COUNTER: AtomicU32 = AtomicU32::new(0);
/// The stamp for this run's `screenshots/<stamp>/` subfolder, taken on the first capture so every
/// screenshot of the run shares one folder.
static SCREENSHOT_STAMP: OnceLock<String> = OnceLock::new();

fn capture(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Resource,
) -> anyhow::Result<PathBuf> {
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
        let src = mapped.pData as *const u8;
        let mut pixels = vec![0u8; width * height * 4];
        for y in 0..height {
            let row = src.add(y * row_pitch);
            for x in 0..width {
                let px = row.add(x * 4);
                let out = (y * width + x) * 4;
                // Store RGBA; swap R/B for the BGRA back-buffer formats. Alpha is passed through.
                let (r, b) = if bgra { (2, 0) } else { (0, 2) };
                pixels[out] = *px.add(r);
                pixels[out + 1] = *px.add(1);
                pixels[out + 2] = *px.add(b);
                pixels[out + 3] = *px.add(3);
            }
        }
        context.Unmap(&staging, 0);

        // All of a run's screenshots share one stamped subfolder (`screenshots/<stamp>/`), mirroring
        // the render-trace layout; the stamp is taken once, on the first capture.
        let stamp = SCREENSHOT_STAMP.get_or_init(crate::session::stamp);
        let dir = crate::session::subdir("screenshots")
            .map(|base| base.join(stamp))
            .context("could not resolve the session screenshots directory")?;
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("jc3vrs-{n:04}.png"));

        let file =
            std::fs::File::create(&path).with_context(|| format!("creating {}", path.display()))?;
        PngEncoder::new(std::io::BufWriter::new(file))
            .write_image(
                &pixels,
                width as u32,
                height as u32,
                ExtendedColorType::Rgba8,
            )
            .with_context(|| format!("encoding {}", path.display()))?;

        // Sidecar JSON: everything relevant about this frame's single-pass state (the per-eye cb13
        // matrices, the centre transform, the viewport, the config), so the exact matrices that
        // produced the image can be inspected offline instead of read from a log line.
        let sidecar = serde_json::json!({
            "image": path.file_name().and_then(|f| f.to_str()),
            "width": width,
            "height": height,
            "format": format!("{:?}", desc.Format),
            "single_pass": serde_json::to_value(crate::stereo::single_pass::last_frame_diagnostics())
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
}
