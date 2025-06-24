pub mod args;
pub mod db;
pub mod file_cache;
pub mod ignore_config;
pub mod move_heuristics;
pub mod platform;
pub mod watcher;
pub mod windows_registry;

#[cfg(test)]
mod test_import {
	use super::ignore_config::IgnoreConfig;
}

#[allow(dead_code)]
fn main() {
	// Your code here
}
