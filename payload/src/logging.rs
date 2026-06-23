use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};

pub(super) fn install() {
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(tracing_subscriber::filter::LevelFilter::INFO.into());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                // No console is allocated, so stdout is not a TTY; keep ANSI off explicitly rather
                // than relying on auto-detection.
                .with_ansi(false)
                .with_filter(env_filter.clone()),
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
                        .with_filter(env_filter)
                }),
        )
        .init();
}

pub(super) fn uninstall() {}
