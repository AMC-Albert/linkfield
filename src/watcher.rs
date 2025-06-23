// File system watcher and event handling logic will be moved here

use crate::file_cache::FileCache;
use crate::move_heuristics::{FileEventKind, MoveHeuristics, make_file_event};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn start_watcher<P: AsRef<Path>>(
    watch_path: P,
    file_cache: Arc<Mutex<FileCache>>,
    heuristics: Arc<Mutex<MoveHeuristics>>,
) -> std::io::Result<()> {
    let watch_path = watch_path.as_ref().to_path_buf();
    println!("[Watcher] Watching directory: {watch_path:?}");
    println!("[Watcher] Initializing watcher in background thread...");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    let heuristics_thread = heuristics;
    let file_cache_thread = file_cache;
    std::thread::spawn(move || {
        use std::collections::HashSet;
        let mut recently_moved: HashSet<std::path::PathBuf> = HashSet::new();
        let mut debouncer =
            notify_debouncer_full::new_debouncer(Duration::from_millis(500), None, tx)
                .map_err(std::io::Error::other)
                .expect("Failed to create debouncer");
        debouncer
            .watch(
                &watch_path,
                notify_debouncer_full::notify::RecursiveMode::Recursive,
            )
            .map_err(std::io::Error::other)
            .expect("Failed to start watcher");
        // Signal ready after watcher is set up
        ready_tx.send(()).ok();
        println!("[WatcherThread] Event loop started");
        for result in rx {
            match result {
                Ok(events) => {
                    for event in events {
                        match &event.event.kind {
                            notify_debouncer_full::notify::event::EventKind::Remove(_) => {
                                let path = event.event.paths.first().cloned();
                                if let Some(path) = path {
                                    let meta =
                                        file_cache_thread.lock().unwrap().get(&path).cloned();
                                    let file_event =
                                        make_file_event(path.clone(), FileEventKind::Remove, meta);
                                    heuristics_thread.lock().unwrap().add_remove(file_event);
                                    file_cache_thread.lock().unwrap().remove_file(&path);
                                }
                                continue;
                            }
                            notify_debouncer_full::notify::event::EventKind::Create(_) => {
                                let path = event.event.paths.first().cloned();
                                if let Some(path) = path {
                                    file_cache_thread.lock().unwrap().update_file(&path);
                                    let meta =
                                        file_cache_thread.lock().unwrap().get(&path).cloned();
                                    let file_event =
                                        make_file_event(path.clone(), FileEventKind::Create, meta);
                                    if let Some(pair) =
                                        heuristics_thread.lock().unwrap().pair_create(&file_event)
                                    {
                                        println!(
                                            "[Heuristics] Move detected: {:?} -> {:?} (score: {:.2})",
                                            pair.from.path, pair.to.path, pair.score
                                        );
                                        recently_moved.insert(pair.to.path.clone());
                                        continue;
                                    }
                                    println!("[Watcher] Create: {path:?}");
                                }
                            }
                            notify_debouncer_full::notify::event::EventKind::Modify(
                                notify_debouncer_full::notify::event::ModifyKind::Name(_),
                            ) => {
                                let paths = &event.event.paths;
                                match paths.len() {
                                    2 => {
                                        let from = &paths[0];
                                        let to = &paths[1];
                                        let old_parent = from.parent();
                                        let new_parent = to.parent();
                                        if old_parent == new_parent {
                                            println!("[Watcher] Rename: {from:?} -> {to:?}");
                                        } else {
                                            println!("[Watcher] Move: {from:?} -> {to:?}");
                                        }
                                        file_cache_thread.lock().unwrap().remove_file(from);
                                        file_cache_thread.lock().unwrap().update_file(to);
                                        recently_moved.insert(to.clone());
                                    }
                                    1 => {
                                        println!(
                                            "[Watcher] Rename/Move event (single path): {:?}",
                                            paths[0]
                                        );
                                    }
                                    _ => {
                                        println!(
                                            "[Watcher] Rename/Move event with unexpected paths: {paths:?}"
                                        );
                                    }
                                }
                                continue;
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
                                    continue;
                                }
                                println!("[Watcher] Event: {event:?}");
                            }
                        }
                    }
                }
                Err(e) => println!("[Watcher] Error: {e:?}"),
            }
        }
    });
    ready_rx
        .recv()
        .expect("Watcher thread failed to initialize");
    println!("[Watcher] Ready. Try renaming, creating, or deleting files in this directory.");
    Ok(())
}
