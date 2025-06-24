// File system watcher and event handling logic will be moved here

use crate::file_cache::FileCache;
use crate::ignore_config::IgnoreConfig;
use crate::move_heuristics::{FileEventKind, MoveHeuristics, make_file_event};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::info;

pub fn start_watcher<P: AsRef<Path>>(
	watch_path: P,
	file_cache: Arc<Mutex<Arc<FileCache>>>,
	heuristics: Arc<Mutex<MoveHeuristics>>,
	ignore_config: Arc<IgnoreConfig>,
) {
	let watch_path = watch_path.as_ref().to_path_buf();
	info!("Watching directory: {}", watch_path.display());
	info!("Initializing watcher...");
	let (ready_tx, ready_rx) = std::sync::mpsc::channel();
	let (tx, rx) = std::sync::mpsc::channel();
	let heuristics_thread = heuristics;
	let file_cache_thread = file_cache;
	let watcher_setup_start = std::time::Instant::now();
	std::thread::spawn(move || {
		use std::collections::HashSet;
		let mut recently_moved: HashSet<std::path::PathBuf> = HashSet::new();
		let mut debouncer =
			match notify_debouncer_full::new_debouncer(Duration::from_millis(500), None, tx) {
				Ok(d) => d,
				Err(e) => {
					tracing::error!("Failed to create debouncer: {e}");
					return;
				}
			};
		if let Err(e) = debouncer
			.watch(
				&watch_path,
				notify_debouncer_full::notify::RecursiveMode::Recursive,
			)
			.map_err(std::io::Error::other)
		{
			tracing::error!("Failed to start watcher: {e}");
			return;
		}
		// Signal ready after watcher is set up
		if ready_tx.send(()).is_err() {
			tracing::error!("Failed to signal ready");
			return;
		}
		let setup_elapsed = watcher_setup_start.elapsed();
		info!(
			"[WatcherThread] Event loop started (setup took {:.2?})",
			setup_elapsed
		);
		for result in rx {
			match result {
				Ok(events) => {
					for event in events {
						// Skip events for paths matching ignore_config
						if event
							.event
							.paths
							.iter()
							.any(|p| ignore_config.is_ignored(p))
						{
							continue;
						}
						handle_event(
							&event,
							&file_cache_thread,
							&heuristics_thread,
							&mut recently_moved,
						);
					}
				}
				Err(e) => tracing::warn!("Watcher error: {e:?}"),
			}
		}
	});
	if let Err(e) = ready_rx.recv() {
		tracing::error!("Watcher thread failed to initialize: {e}");
		return;
	}
	info!("Watcher ready. Try renaming, creating, or deleting files in this directory.");
}

fn handle_remove_event(
	event: &notify_debouncer_full::DebouncedEvent,
	file_cache_thread: &Arc<Mutex<Arc<FileCache>>>,
	heuristics_thread: &Arc<Mutex<MoveHeuristics>>,
) {
	let path = event.event.paths.first().cloned();
	if let Some(path) = path {
		let meta = match file_cache_thread.lock() {
			Ok(guard) => guard.get(&path).map(|m| m.clone()),
			Err(e) => {
				tracing::error!(error = %e, "Failed to lock file_cache");
				None
			}
		};
		let file_event = make_file_event(path.clone(), FileEventKind::Remove, meta);
		if let Ok(mut heuristics) = heuristics_thread.lock() {
			heuristics.add_remove(file_event);
		} else {
			tracing::error!("Failed to lock heuristics for remove");
		}
		if let Ok(cache) = file_cache_thread.lock() {
			cache.remove_file(&path);
		} else {
			tracing::error!("Failed to lock file_cache for remove_file");
		}
	}
}

fn handle_create_event(
	event: &notify_debouncer_full::DebouncedEvent,
	file_cache_thread: &Arc<Mutex<Arc<FileCache>>>,
	heuristics_thread: &Arc<Mutex<MoveHeuristics>>,
	recently_moved: &mut std::collections::HashSet<std::path::PathBuf>,
) {
	let path = event.event.paths.first().cloned();
	if let Some(path) = path {
		if let Ok(cache) = file_cache_thread.lock() {
			cache.update_file(&path);
		} else {
			tracing::error!("Failed to lock file_cache for update_file");
		}
		let meta = match file_cache_thread.lock() {
			Ok(guard) => guard.get(&path).map(|m| m.clone()),
			Err(e) => {
				tracing::error!(error = %e, "Failed to lock file_cache");
				None
			}
		};
		let file_event = make_file_event(path.clone(), FileEventKind::Create, meta);
		let pair = match heuristics_thread.lock() {
			Ok(mut heuristics) => heuristics.pair_create(&file_event),
			Err(e) => {
				tracing::error!(error = %e, "Failed to lock heuristics for pair_create");
				None
			}
		};
		if let Some(pair) = pair {
			tracing::info!(from = %pair.from.path.display(), to = %pair.to.path.display(), score = pair.score, "Move detected");
			recently_moved.insert(pair.to.path);
			return;
		}
		tracing::info!(path = %path.display(), "Create");
	}
}

fn handle_modify_name_event(
	event: &notify_debouncer_full::DebouncedEvent,
	file_cache_thread: &Arc<Mutex<Arc<FileCache>>>,
	recently_moved: &mut std::collections::HashSet<std::path::PathBuf>,
) {
	let paths = &event.event.paths;
	match paths.len() {
		2 => {
			let from = &paths[0];
			let to = &paths[1];
			let old_parent = from.parent();
			let new_parent = to.parent();
			if old_parent == new_parent {
				tracing::info!(from = %from.display(), to = %to.display(), "Rename");
			} else {
				tracing::info!(from = %from.display(), to = %to.display(), "Move");
			}
			if let Ok(cache) = file_cache_thread.lock() {
				cache.remove_file(from);
				cache.update_file(to);
			} else {
				tracing::error!("Failed to lock file_cache for rename/move");
			}
			recently_moved.insert(to.clone());
		}
		1 => {
			tracing::info!(path = %paths[0].display(), "Rename/Move event (single path)");
		}
		_ => {
			tracing::info!(?paths, "Rename/Move event with unexpected paths");
		}
	}
}

fn handle_event(
	event: &notify_debouncer_full::DebouncedEvent,
	file_cache_thread: &Arc<Mutex<Arc<FileCache>>>,
	heuristics_thread: &Arc<Mutex<MoveHeuristics>>,
	recently_moved: &mut std::collections::HashSet<std::path::PathBuf>,
) {
	match &event.event.kind {
		notify_debouncer_full::notify::event::EventKind::Remove(_) => {
			handle_remove_event(event, file_cache_thread, heuristics_thread);
		}
		notify_debouncer_full::notify::event::EventKind::Create(_) => {
			handle_create_event(event, file_cache_thread, heuristics_thread, recently_moved);
		}
		notify_debouncer_full::notify::event::EventKind::Modify(
			notify_debouncer_full::notify::event::ModifyKind::Name(_),
		) => {
			handle_modify_name_event(event, file_cache_thread, recently_moved);
		}
		_ => {
			let paths = &event.event.paths;
			let is_dir_event = paths.iter().any(|p| {
				p.ends_with("linkfield.redb")
					|| std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
					|| recently_moved.remove(p)
			});
			if matches!(
				&event.event.kind,
				notify_debouncer_full::notify::event::EventKind::Modify(
					notify_debouncer_full::notify::event::ModifyKind::Any,
				)
			) && is_dir_event
			{
				return;
			}
			tracing::info!(?event, "Event");
		}
	}
}
