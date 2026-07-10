//! File-only logging. This app has no console, so the log file is the only diagnostic.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Keep the returned guard alive for the process lifetime; dropping it stops the writer.
pub fn init(dir: &Path) -> WorkerGuard {
    let appender = tracing_appender::rolling::never(dir, "log.txt");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));

    guard
}
