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
        .with_writer(|| {
            struct AutoFlushStdout;
            impl std::io::Write for AutoFlushStdout {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    let n = std::io::stdout().write(buf)?;
                    std::io::stdout().flush()?;
                    Ok(n)
                }
                fn flush(&mut self) -> std::io::Result<()> {
                    std::io::stdout().flush()
                }
            }
            AutoFlushStdout
        })
        .init();
    app::run()
}
