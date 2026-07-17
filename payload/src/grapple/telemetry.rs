//! The grapple telemetry capture: a CSV stream of the filter's inputs and outputs, for offline
//! analysis of comfort issues that are hard to judge from inside the headset.
//!
//! While enabled (from the debug UI's Camera tab; off by default), one row is written per input
//! tick (`tick`) and per rendered VR frame (`frame`) to a timestamped
//! `jc3vrs-grapple-<yyyymmdd-hhmmss>.csv` beside the payload DLL. The file is created lazily on
//! the first row, and toggling off and on starts a fresh file, so captures with different settings
//! stay separate. A dedicated file rather than the tracing stack because the capture is a dense
//! machine-read stream that must survive log-filter changes and re-injection.

use std::{
    io::Write as _,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use super::filter_snapshot;

/// Whether the capture is running.
pub fn log_enabled() -> bool {
    LOG_ENABLED.load(Ordering::Relaxed)
}

/// Start or stop the capture (see the module docs).
pub fn set_log_enabled(enabled: bool) {
    if !enabled && TELEMETRY.lock().take().is_some() {
        tracing::info!("grapple telemetry capture stopped");
    }
    LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Log one input tick's filter state (no-op when disabled). Called from
/// [`crate::headpose::sim::on_input_tick`] after [`super::advance`]; `dt` is the engine's tick
/// delta.
pub fn log_tick(body: Quat, dt: f32) {
    if !log_enabled() {
        return;
    }
    let (mode, blend, held) = filter_snapshot();
    let fields = format!(
        "{state},{mode:?},{blend:.4},{dt:.5},{raw},{filt},{held},{empty}",
        state = super::hook_snapshot().map_or_else(
            || "None".to_string(),
            |h| format!(
                "{:?}{}{}",
                h.state,
                if h.wire { "+wire" } else { "-wire" },
                if h.firing { "+fire" } else { "" },
            )
        ),
        blend = blend,
        raw = csv_quat(body),
        filt = csv_quat(super::filter_with(body, mode, held, blend)),
        held = csv_quat(held),
        empty = ",".repeat(16),
    );
    write_row("tick", &fields);
}

/// One rendered VR frame's pose composition, for [`log_frame`]: the HMD cockpit pose, the raw and
/// filtered body frames the frame used, and the composed head pose it published.
pub struct FrameTelemetry {
    pub cockpit_orientation: Quat,
    pub cockpit_position: Vec3,
    pub body_raw: Quat,
    pub body_filtered: Quat,
    pub composed: Quat,
    pub position: Vec3,
    pub anchor: Vec3,
}

/// Log one rendered VR frame's pose composition (no-op when disabled). Called from the VR frame
/// loop after the pose pair publishes.
pub fn log_frame(frame: &FrameTelemetry) {
    if !log_enabled() {
        return;
    }
    let (mode, blend, held) = filter_snapshot();
    let fields = format!(
        ",{mode:?},{blend:.4},,{raw},{filt},{held},{cockpit},{cockpit_pos},{composed},{pos},{anchor}",
        raw = csv_quat(frame.body_raw),
        filt = csv_quat(frame.body_filtered),
        held = csv_quat(held),
        cockpit = csv_quat(frame.cockpit_orientation),
        cockpit_pos = csv_vec(frame.cockpit_position),
        composed = csv_quat(frame.composed),
        pos = csv_vec(frame.position),
        anchor = csv_vec(frame.anchor),
    );
    write_row("frame", &fields);
}

static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// The active capture, or `None` when disabled.
static TELEMETRY: Mutex<Option<TelemetryWriter>> = Mutex::new(None);

struct TelemetryWriter {
    file: std::fs::File,
    /// The capture start, the zero of the rows' `t` column.
    started: Instant,
}

/// The CSV column header. `tick` rows fill through `held_*` and leave the frame columns empty;
/// `frame` rows leave `state`/`dt` empty and fill the rest. All quaternions are `x,y,z,w` in world
/// space (the cockpit pose is relative to the recenter baseline); positions are metres.
const TELEMETRY_HEADER: &str = concat!(
    "kind,t,state,mode,blend,dt,",
    "raw_qx,raw_qy,raw_qz,raw_qw,",
    "filt_qx,filt_qy,filt_qz,filt_qw,",
    "held_qx,held_qy,held_qz,held_qw,",
    "cockpit_qx,cockpit_qy,cockpit_qz,cockpit_qw,cockpit_px,cockpit_py,cockpit_pz,",
    "composed_qx,composed_qy,composed_qz,composed_qw,",
    "pos_x,pos_y,pos_z,anchor_x,anchor_y,anchor_z\n",
);

/// Append one row to the capture: `kind,t,<fields>`, opening the timestamped file on the first
/// row. Errors stop the capture rather than spamming.
fn write_row(kind: &str, fields: &str) {
    let mut writer = TELEMETRY.lock();
    if writer.is_none() {
        *writer = open_capture();
        if writer.is_none() {
            LOG_ENABLED.store(false, Ordering::Relaxed);
            return;
        }
    }
    let Some(w) = writer.as_mut() else {
        return;
    };
    let t = w.started.elapsed().as_secs_f64();
    let row = format!("{kind},{t:.4},{fields}\n");
    if let Err(e) = w.file.write_all(row.as_bytes()) {
        tracing::warn!(error = %e, "grapple telemetry: write failed; capture stopped");
        *writer = None;
        LOG_ENABLED.store(false, Ordering::Relaxed);
    }
}

/// Create a fresh timestamped capture file with its header, or `None` (with a warning logged) when
/// the path cannot be resolved or created.
fn open_capture() -> Option<TelemetryWriter> {
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string();
    let Some(path) = crate::module::get_path()
        .as_ref()
        .and_then(|path| path.parent())
        .map(|parent| parent.join(format!("jc3vrs-grapple-{stamp}.csv")))
    else {
        tracing::warn!("grapple telemetry: could not resolve the payload module path");
        return None;
    };
    match std::fs::File::create(&path) {
        Ok(mut file) => {
            let _ = file.write_all(TELEMETRY_HEADER.as_bytes());
            tracing::info!(path = %path.display(), "grapple telemetry capture started");
            Some(TelemetryWriter {
                file,
                started: Instant::now(),
            })
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "grapple telemetry: create failed");
            None
        }
    }
}

/// A quaternion as four CSV fields (`x,y,z,w`).
fn csv_quat(q: Quat) -> String {
    format!("{:.6},{:.6},{:.6},{:.6}", q.x, q.y, q.z, q.w)
}

/// A vector as three CSV fields.
fn csv_vec(v: Vec3) -> String {
    format!("{:.4},{:.4},{:.4}", v.x, v.y, v.z)
}
