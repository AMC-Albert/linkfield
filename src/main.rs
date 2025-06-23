#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

mod app;
mod args;
mod db;
mod file_cache;
mod move_heuristics;
mod platform;
mod watcher;
mod windows_registry;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::fmt::format::FmtSpan;
    tracing_subscriber::fmt()
        .with_ansi(true)
        .with_level(true)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .without_time()
        .with_span_events(FmtSpan::NONE)
        .compact()
        .init();
    app::run()
}
