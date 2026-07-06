//! Logging: stdout + a `jc3vrs.log` beside the payload DLL, filtered by `RUST_LOG` from the game
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
            crate::module::get_path()
                .as_ref()
                .and_then(|path| path.parent())
                .map(|parent| parent.join("jc3vrs.log"))
                .and_then(|path| std::fs::File::create(&path).ok())
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
