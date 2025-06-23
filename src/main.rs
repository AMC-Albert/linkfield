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
    app::run()
}
