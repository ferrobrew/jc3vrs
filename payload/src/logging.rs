//! Logging: stdout + a `jc3vr.log` in the session's `log/` directory (see [`crate::session`]),
//! filtered by `RUST_LOG` from the game
//! process's environment (fixed at game launch), with a live reload path so the filter can be
//! changed from the debug UI without relaunching the game.

use std::sync::OnceLock;

use parking_lot::Mutex;
use tracing_subscriber::{
    EnvFilter, Layer as _, layer::SubscriberExt as _, reload, util::SubscriberInitExt as _,
};

/// Replace the active log filter with `spec` (standard `RUST_LOG` directive syntax, e.g.
/// `warn,vr=debug,coord_frame=debug`). Applies to both the stdout and file layers. Returns a
/// user-displayable error when the spec does not parse or logging is not installed.
pub fn set_filter(spec: &str) -> Result<(), String> {
    let handles = RELOAD_HANDLES
        .get()
        .ok_or_else(|| "logging: not installed yet".to_string())?;
    for handle in handles {
        let filter = EnvFilter::try_new(spec)
            .map_err(|e| format!("logging: invalid filter spec {spec:?}: {e}"))?;
        handle(filter).map_err(|e| format!("logging: filter reload failed: {e}"))?;
    }
    *ACTIVE_SPEC.lock() = Some(spec.to_string());
    tracing::info!(spec, "log filter replaced from the debug UI");
    Ok(())
}

/// The spec applied via [`set_filter`], or `None` while the launch environment's `RUST_LOG` (with
/// the INFO floor) is still in effect.
pub fn active_spec() -> Option<String> {
    ACTIVE_SPEC.lock().clone()
}

/// Turn a single target's `DEBUG` output on or off, leaving the rest of the active spec alone. This
/// backs the debug UI's per-subsystem verbosity checkboxes: composing a directive by hand is fine at
/// a desk but not in a headset, where there is no comfortable keyboard.
///
/// Any existing directive for the same target is replaced. If no spec has been applied yet, the
/// launch environment's `RUST_LOG` cannot be read back as a string, so the base becomes the plain
/// INFO floor — a launch-time `RUST_LOG` is dropped the first time a checkbox is used.
pub fn set_target_debug(target: &str, on: bool) -> Result<(), String> {
    let prefix = format!("{target}=");
    let base = active_spec().unwrap_or_else(|| DEFAULT_LEVEL.to_string());
    let mut directives: Vec<&str> = base
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty() && !d.starts_with(&prefix))
        .collect();
    let enabled = format!("{target}=debug");
    if on {
        directives.push(&enabled);
    }
    if directives.is_empty() {
        directives.push(DEFAULT_LEVEL);
    }
    set_filter(&directives.join(","))
}

/// Whether the active spec turns `target` up to `DEBUG` (see [`set_target_debug`]).
pub fn is_target_debug(target: &str) -> bool {
    active_spec().is_some_and(|spec| {
        spec.split(',')
            .any(|d| d.trim() == format!("{target}=debug"))
    })
}

/// The level the filter falls back to with no other directive: the same floor [`initial_filter`]
/// installs.
const DEFAULT_LEVEL: &str = "info";

pub(super) fn install() {
    let (stdout_filter, stdout_handle) = reload::Layer::new(initial_filter());
    let (file_filter, file_handle) = reload::Layer::new(initial_filter());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                // No console is allocated, so stdout is not a TTY; keep ANSI off explicitly rather
                // than relying on auto-detection.
                .with_ansi(false)
                .with_filter(stdout_filter),
        )
        .with(
            crate::session::dir()
                .and_then(|r| r.ok())
                .map(|dir| dir.join("jc3vr.log"))
                .and_then(|path| match std::fs::File::create(&path) {
                    Ok(file) => Some(file),
                    Err(e) => {
                        eprintln!("logging: could not open {}: {e}", path.display());
                        None
                    }
                })
                .map(|file| {
                    tracing_subscriber::fmt::layer()
                        // Never write ANSI escapes to the log file.
                        .with_ansi(false)
                        .with_writer(file)
                        .with_filter(file_filter)
                }),
        )
        .init();

    // The handles are stored as type-erased closures: `reload::Handle` is generic over the layered
    // subscriber type at its position, which is unnameable here without repeating the whole stack.
    RELOAD_HANDLES
        .set(vec![
            Box::new(move |f| stdout_handle.reload(f)),
            Box::new(move |f| file_handle.reload(f)),
        ])
        .ok();
}

pub(super) fn uninstall() {}

/// The filter in effect until [`set_filter`] replaces it: the launch environment's `RUST_LOG`
/// directives over an INFO floor.
fn initial_filter() -> EnvFilter {
    EnvFilter::from_default_env()
        .add_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
}

/// One reload closure per installed layer; applying a new filter reloads every layer.
type ReloadHandle = Box<dyn Fn(EnvFilter) -> Result<(), reload::Error> + Send + Sync>;

static RELOAD_HANDLES: OnceLock<Vec<ReloadHandle>> = OnceLock::new();

/// The last spec applied via [`set_filter`]; `None` means the launch environment is in effect.
static ACTIVE_SPEC: Mutex<Option<String>> = Mutex::new(None);
