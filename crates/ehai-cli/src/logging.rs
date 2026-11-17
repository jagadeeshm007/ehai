//! JSON file logs in `~/.ehai/logs` (daily rotation, 14 kept), plus human-readable
//! stderr when `-v` is passed. Logging problems never stop the CLI.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub fn init(logs_dir: &Path, verbose: u8) -> Option<WorkerGuard> {
    let file = std::fs::create_dir_all(logs_dir)
        .ok()
        .and_then(|()| {
            Builder::new()
                .rotation(Rotation::DAILY)
                .filename_prefix("ehai")
                .filename_suffix("log")
                .max_log_files(14)
                .build(logs_dir)
                .ok()
        })
        .map(tracing_appender::non_blocking);
    let (file_layer, guard) = match file {
        Some((writer, guard)) => (
            Some(fmt::layer().json().with_writer(writer).with_filter(
                EnvFilter::try_from_env("EHAI_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
            )),
            Some(guard),
        ),
        None => (None, None),
    };
    let stderr_level = match verbose {
        0 => None,
        1 => Some("info"),
        2 => Some("debug"),
        _ => Some("trace"),
    };
    let stderr_layer = stderr_level.map(|lvl| {
        fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .with_filter(EnvFilter::new(lvl))
    });
    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
    if guard.is_none() {
        eprintln!(
            "warning: could not open log dir {}; file logging disabled",
            logs_dir.display()
        );
    }
    guard
}
