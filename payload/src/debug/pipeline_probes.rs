//! Pipeline brightness and constant probes: where in the frame does a global change enter?
//!
//! Three trace instruments distilled from the foveation blend-state hunt, plus the auto-capture
//! that arms them:
//!
//! - **Stage brackets** ([`record_main_color_mean`], `stereo.diagnose_main_color_means`): the
//!   subsampled MainColor mean at fixed pipeline seams (post-resolve, around the aerial-perspective
//!   composite, post-block entry, post-chain start). A change bracketed between two stages was
//!   injected by the work between them.
//! - **The per-pass ladder** ([`record_pass_sweep_mean`], `stereo.diagnose_pass_sweep`): the same
//!   mean after every late-scene render pass, walking a change to the exact pass.
//! - **Constant dumps** ([`record_global_constants`], `stereo.diagnose_global_constants`): the
//!   FP/VP global constant staging blocks at scene time and frame end, for good-vs-bad diffing.
//! - **Auto-capture** ([`armed_brightness_probe`], `stereo.auto_trace_brightness_step`): a one-shot
//!   trigger that starts a render trace the moment the MainColor mean steps against its recent
//!   median, then disarms.
//!
//! All are no-ops unless their toggle is set (and, for the recorders, a trace is collecting); the
//! readbacks drain the GPU up to the copy, which is the accepted price while diagnosing.

use anyhow::Context as _;
use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use windows::{
    Win32::{
        Graphics::Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_STAGING, ID3D11Texture2D,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface,
};

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState, tracing_active},
};

/// Record the render engine's CPU staging copies of the global shader constant buffers into the
/// active trace, tagged with the sample point. The staging blocks are re-staged per pass, so
/// `scene`-time rows differ from what the post pass leaves for `frame_end`. No-op unless a trace is
/// collecting and `stereo.diagnose_global_constants` is set.
pub fn record_global_constants(at: &'static str) {
    if !tracing_active() || !Config::lock_query(|c| c.stereo.diagnose_global_constants) {
        return;
    }
    let Some(re) = (unsafe { jc3gi::graphics_engine::render_engine::RenderEngine::get() }) else {
        return;
    };
    let flatten = |rows: &[jc3gi::types::math::Vector4]| {
        let mut out = Vec::with_capacity(rows.len() * 4);
        for r in rows {
            out.extend_from_slice(&r.data);
        }
        out
    };
    TraceState::record_eye(TraceEvent::GlobalConstants {
        at: at.to_owned(),
        fp: flatten(&re.m_FPGlobalConstData),
        vp: flatten(&re.m_VPGlobalConstData),
    });
}

/// Record the MainColor mean at a named pipeline stage bracket. No-op unless a trace is collecting
/// and `stereo.diagnose_main_color_means` is set; the readback drains the GPU up to the copy, which
/// is the price of the bracket.
pub fn record_main_color_mean(at: &'static str) {
    if !Config::lock_query(|c| c.stereo.diagnose_main_color_means) {
        return;
    }
    record_mean_labeled(at.to_owned());
}

/// Record the MainColor mean with a caller-built label, for the per-pass sweep. No-op unless a
/// trace is collecting and `stereo.diagnose_pass_sweep` is set.
pub fn record_pass_sweep_mean(at: String) {
    if !Config::lock_query(|c| c.stereo.diagnose_pass_sweep) {
        return;
    }
    record_mean_labeled(at);
}

/// The shared body of the mean probes: read, then record under the label.
fn record_mean_labeled(at: String) {
    if !tracing_active() {
        return;
    }
    match unsafe { read_main_color_mean() } {
        Ok((r, g, b)) => TraceState::record_eye(TraceEvent::MainColorMean { at, r, g, b }),
        Err(e) => tracing::warn!("main-color mean: {e:#}"),
    }
}

/// While the brightness-step auto-capture is armed, probe the MainColor mean once per frame (at the
/// pre-post seam) and start a trace when it jumps against the median of its recent history. Unlike
/// the histogram-divisor signal this reads the flip directly and within one frame; the readback
/// stall is the price of being armed. One-shot: disarms the toggle on trigger.
pub fn armed_brightness_probe() {
    const HISTORY: usize = 30;
    const MIN_HISTORY: usize = 10;
    const RATIO_UP: f32 = 1.35;
    const RATIO_DOWN: f32 = 0.74;
    static MEANS: parking_lot::Mutex<std::collections::VecDeque<f32>> =
        parking_lot::Mutex::new(std::collections::VecDeque::new());

    if !Config::lock_query(|c| c.stereo.auto_trace_brightness_step) || tracing_active() {
        MEANS.lock().clear();
        return;
    }
    let Ok((r, g, b)) = (unsafe { read_main_color_mean() }) else {
        return;
    };
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    if !lum.is_finite() || lum <= 0.0 {
        return;
    }
    let mut means = MEANS.lock();
    let triggered = if means.len() >= MIN_HISTORY {
        let mut sorted: Vec<f32> = means.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        median > 0.0 && (lum > median * RATIO_UP || lum < median * RATIO_DOWN)
    } else {
        false
    };
    if triggered {
        means.clear();
        drop(means);
        crate::config::CONFIG
            .lock()
            .stereo
            .auto_trace_brightness_step = false;
        let frames = crate::ui::diagnostics::TRACE_FRAME_COUNT
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(30);
        tracing::info!(
            "brightness-step auto-capture: MainColor mean jumped, tracing {frames} frames"
        );
        TraceState::start(frames);
    } else {
        if means.len() >= HISTORY {
            means.pop_front();
        }
        means.push_back(lum);
    }
}

/// Copy MainColor to a staging texture and return its subsampled linear mean.
///
/// # Safety
/// Dereferences engine singletons; the engine device, context, and MainColor must be live.
unsafe fn read_main_color_mean() -> anyhow::Result<(f32, f32, f32)> {
    let ge = unsafe { GraphicsEngine::get() }.context("graphics engine unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("graphics device unavailable")?;
    let context = unsafe { device.m_Context.as_ref() }.context("graphics context unavailable")?;
    let ctx = &context.m_Context;
    let tex = unsafe { ge.m_MainColorBuffer.as_ref() }.context("MainColor unavailable")?;
    let src2d: ID3D11Texture2D = tex.m_Texture.cast().context("MainColor is not 2D")?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { src2d.GetDesc(&mut desc) };
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..desc
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe {
        device
            .m_Device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))
    }
    .context("creating a MainColor staging texture")?;
    let staging = staging.context("MainColor staging texture not created")?;
    let mut sums = [0.0f64; 3];
    let mut n = 0usize;
    unsafe {
        EnterCriticalSection(context.m_Mutex);
        ctx.CopyResource(&staging, &tex.m_Texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let mapped_ok = ctx
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .is_ok();
        if mapped_ok {
            let pitch = mapped.RowPitch as usize;
            let base = mapped.pData.cast::<u8>();
            for y in (0..desc.Height as usize).step_by(8) {
                for x in (0..desc.Width as usize).step_by(8) {
                    let px = base.add(y * pitch + x * 4).cast::<u32>().read_unaligned();
                    let (r, g, b) = decode_r11g11b10(px);
                    sums[0] += r as f64;
                    sums[1] += g as f64;
                    sums[2] += b as f64;
                    n += 1;
                }
            }
            ctx.Unmap(&staging, 0);
        }
        LeaveCriticalSection(context.m_Mutex);
        if !mapped_ok {
            anyhow::bail!("mapping the MainColor staging texture failed");
        }
    }
    if n == 0 {
        anyhow::bail!("MainColor sampling covered no pixels");
    }
    Ok((
        (sums[0] / n as f64) as f32,
        (sums[1] / n as f64) as f32,
        (sums[2] / n as f64) as f32,
    ))
}

/// Decode one packed `R11G11B10_FLOAT` texel into linear RGB. The 11-bit channels carry a 5-bit
/// exponent and 6-bit mantissa, the 10-bit channel 5/5, all unsigned with a half-float-style bias.
fn decode_r11g11b10(px: u32) -> (f32, f32, f32) {
    fn small_float(bits: u32, mant_bits: u32) -> f32 {
        let exp = (bits >> mant_bits) & 0x1F;
        let mant = bits & ((1 << mant_bits) - 1);
        let scale = (1u32 << mant_bits) as f32;
        if exp == 0 {
            (mant as f32 / scale) * 2f32.powi(-14)
        } else if exp == 31 {
            f32::INFINITY
        } else {
            (1.0 + mant as f32 / scale) * 2f32.powi(exp as i32 - 15)
        }
    }
    (
        small_float(px & 0x7FF, 6),
        small_float((px >> 11) & 0x7FF, 6),
        small_float((px >> 22) & 0x3FF, 5),
    )
}
