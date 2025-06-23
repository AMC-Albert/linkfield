use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::args;
use crate::db;
use crate::file_cache::FileCache;
use crate::move_heuristics::MoveHeuristics;
use crate::platform;
use crate::watcher;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    platform::handle_platform_startup();
    println!("[main] Starting linkfield");
    std::io::stdout().flush()?;
    let (db_path_buf, watch_root_buf) = args::parse_args();
    let db_path = db_path_buf.as_path();
    let watch_root = watch_root_buf.as_path();
    println!("[main] db_path: {db_path:?}, watch_root: {watch_root:?}");
    std::io::stdout().flush()?;
    let db = db::open_or_create_db(db_path)?;
    println!("[main] Opened/created redb file");
    std::io::stdout().flush()?;
    println!("[main] Ensuring file_cache table exists...");
    std::io::stdout().flush()?;
    db::ensure_file_cache_table(&db)?;
    println!("[main] file_cache table ready");
    std::io::stdout().flush()?;
    let file_cache = Arc::new(Mutex::new(FileCache::with_redb(db)));
    let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
    println!("[main] Created FileCache and Heuristics");
    std::io::stdout().flush()?;
    if let Ok(mut cache) = file_cache.lock() {
        cache.load_from_redb();
    } else {
        eprintln!("[main] ERROR: failed to lock file_cache for load_from_redb");
    }
    println!("[main] Loaded file cache from redb");
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
    watcher::start_watcher(watch_root, file_cache.clone(), heuristics)?;
    println!("[main] Started watcher");
    std::io::stdout().flush()?;
    if let Ok(cache) = file_cache.lock() {
        let count = cache.all_files().count();
        let total_size: u64 = cache.all_files().map(|m| m.size).sum();
        println!("[FileCache] Initial files cached: {count} (total size: {total_size} bytes)");
    } else {
        eprintln!("[main] ERROR: failed to lock file_cache for stats");
    }
    platform::wait_for_exit();
    Ok(())
}
