use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use windows::Win32::System::Console::{
    AllocConsole, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, FreeConsole,
    GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
};

pub(super) fn install() {
    unsafe {
        AllocConsole().ok();
        if let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) {
            SetConsoleMode(
                handle,
                ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT,
            )
            .ok();
        }
    }

    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(tracing_subscriber::filter::LevelFilter::INFO.into());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
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
                        .with_writer(file)
                        .with_ansi(false)
                        .with_filter(env_filter)
                }),
        )
        .init();
}

pub(super) fn uninstall() {
    unsafe {
        FreeConsole().ok();
    }
}
