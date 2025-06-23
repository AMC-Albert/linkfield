#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]

use redb::Builder;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

mod file_cache;
use file_cache::FileCache;
mod move_heuristics;
use move_heuristics::{FileEventKind, MoveHeuristics, make_file_event};
use std::sync::{Arc, Mutex};
mod windows_registry;

fn start_watcher<P: AsRef<Path>>(
    watch_path: P,
    file_cache: Arc<Mutex<FileCache>>,
    heuristics: Arc<Mutex<MoveHeuristics>>,
) -> std::io::Result<()> {
    let watch_path = watch_path.as_ref().to_path_buf();
    println!("[Watcher] Watching directory: {:?}", watch_path);
    println!("[Watcher] Initializing watcher in background thread...");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel();
    let heuristics_thread = heuristics.clone();
    let file_cache_thread = file_cache.clone();
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
                                // Suppress Remove log for move detection
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
                                    } else {
                                        println!("[Watcher] Create: {:?}", path);
                                    }
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
                                        if old_parent != new_parent {
                                            println!("[Watcher] Move: {:?} -> {:?}", from, to);
                                        } else {
                                            println!("[Watcher] Rename: {:?} -> {:?}", from, to);
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
                                            "[Watcher] Rename/Move event with unexpected paths: {:?}",
                                            paths
                                        );
                                    }
                                }
                                // Suppress generic event print for all rename/move
                                continue;
                            }
                            _ => {
                                // Only print non-rename/move events, and suppress Modify(Any) for directory, .redb, or recently moved file
                                let paths = &event.event.paths;
                                let is_dir_event = paths.iter().any(|p| {
                                    p.ends_with("linkfield.redb")
                                        || std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
                                        || recently_moved.remove(p)
                                });
                                if let notify_debouncer_full::notify::event::EventKind::Modify(
                                    notify_debouncer_full::notify::event::ModifyKind::Any,
                                ) = &event.event.kind
                                {
                                    if is_dir_event {
                                        continue;
                                    }
                                }
                                println!("[Watcher] Event: {:?}", event);
                            }
                        }
                    }
                }
                Err(e) => println!("[Watcher] Error: {:?}", e),
            }
        }
    });
    // Wait for watcher to be ready
    ready_rx
        .recv()
        .expect("Watcher thread failed to initialize");
    println!("[Watcher] Ready. Try renaming, creating, or deleting files in this directory.");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        use windows_registry::{is_redb_registered, register_redb_extension};
        if !is_redb_registered() {
            if let Err(e) = register_redb_extension(false) {
                eprintln!("[main] Failed to register .redb extension: {e}");
            }
        }
    }
    println!("[main] Starting linkfield");
    std::io::stdout().flush()?;
    let args: Vec<String> = std::env::args().collect();
    // Robustly determine database file to open and directory to watch
    let (db_path_buf, watch_root_buf) = if args.len() > 1 {
        let arg_path = Path::new(&args[1]);
        if arg_path.is_file() {
            (
                arg_path.to_path_buf(),
                arg_path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            )
        } else if arg_path.is_dir() {
            (arg_path.join("linkfield.redb"), arg_path.to_path_buf())
        } else {
            (
                Path::new("test.redb").to_path_buf(),
                Path::new(".").to_path_buf(),
            )
        }
    } else {
        (
            Path::new("test.redb").to_path_buf(),
            Path::new(".").to_path_buf(),
        )
    };
    let db_path = db_path_buf.as_path();
    let watch_root = watch_root_buf.as_path();
    println!(
        "[main] db_path: {:?}, watch_root: {:?}",
        db_path, watch_root
    );
    std::io::stdout().flush()?;
    let db = if db_path.exists() {
        Builder::new()
            .create_with_file_format_v3(true)
            .open(db_path)
            .map_err(|e| {
                eprintln!("Failed to open redb file: {e}");
                e
            })?
    } else {
        Builder::new()
            .create_with_file_format_v3(true)
            .create(db_path)
            .map_err(|e| {
                eprintln!("Failed to create redb file: {e}");
                e
            })?
    };
    println!("[main] Opened/created redb file");
    std::io::stdout().flush()?;
    println!("[main] Ensuring file_cache table exists...");
    std::io::stdout().flush()?;
    // Ensure the file_cache table exists (open_table creates if missing)
    {
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("[main] ERROR: failed to begin write txn: {e}");
                return Err(Box::new(e));
            }
        };
        match write_txn.open_table(file_cache::FILE_CACHE_TABLE) {
            Ok(_) => println!("[main] file_cache table opened/created successfully"),
            Err(e) => {
                println!("[main] ERROR: failed to open/create file_cache table: {e}");
                std::process::exit(1);
            }
        }
        if let Err(e) = write_txn.commit() {
            println!("[main] ERROR: failed to commit table creation: {e}");
            std::process::exit(1);
        }
    }
    println!("[main] file_cache table ready");
    std::io::stdout().flush()?;
    let file_cache = Arc::new(Mutex::new(FileCache::with_redb(db)));
    let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
    println!("[main] Created FileCache and Heuristics");
    std::io::stdout().flush()?;
    // Load cache from redb BEFORE first scan_dir
    if let Ok(mut cache) = file_cache.lock() {
        cache.load_from_redb();
    } else {
        eprintln!("[main] ERROR: failed to lock file_cache for load_from_redb");
    }
    println!("[main] Loaded file cache from redb");
    // File cache scan/logging BEFORE watcher
    println!("[main] About to scan_dir");
    std::io::stdout().flush()?;
    if let Ok(mut cache) = file_cache.lock() {
        cache.scan_dir(watch_root);
        println!(
            "[FileCache] After scan_dir: {} files",
            cache.all_files().count()
        );
    } else {
        eprintln!("[main] ERROR: failed to lock file_cache for scan_dir");
    }
    std::io::stdout().flush()?;
    println!("[main] About to start watcher");
    std::io::stdout().flush()?;
    start_watcher(watch_root, file_cache.clone(), heuristics).map_err(|e| {
        eprintln!("Failed to start watcher: {e}");
        e
    })?;
    println!("[main] Started watcher");
    std::io::stdout().flush()?;

    // Print file cache stats after initial scan
    if let Ok(cache) = file_cache.lock() {
        let count = cache.all_files().count();
        let total_size: u64 = cache.all_files().map(|m| m.size).sum();
        println!(
            "[FileCache] Initial files cached: {} (total size: {} bytes)",
            count, total_size
        );
    } else {
        eprintln!("[main] ERROR: failed to lock file_cache for stats");
    }

    // Block main thread to keep process alive (cross-platform)
    #[cfg(windows)]
    {
        use std::io::{self, Read};
        println!("\nPress Enter to exit...");
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut buf = [0u8; 1];
        loop {
            match handle.read(&mut buf) {
                Ok(n) if n > 0 && buf[0] == b'\n' => break,
                Ok(_) => (),
                Err(e) => {
                    eprintln!("[main] ERROR: stdin read failed: {e}");
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[cfg(not(windows))]
    {
        use std::io::{self, Read};
        println!("\nPress Enter to exit...");
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut buf = [0u8; 1];
        loop {
            match handle.read(&mut buf) {
                Ok(n) if n > 0 && buf[0] == b'\n' => break,
                Ok(_) => (),
                Err(e) => {
                    eprintln!("[main] ERROR: stdin read failed: {e}");
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Ok(())
}
