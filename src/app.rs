use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::args;
use crate::db;
use crate::file_cache::FileCache;
use crate::move_heuristics::MoveHeuristics;
use crate::platform;
use crate::watcher;
use tracing::{info, info_span};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let startup_span = info_span!("app_startup");
    let _startup_enter = startup_span.enter();
    platform::handle_platform_startup();
    info!("Starting linkfield");
    std::io::stdout().flush()?;
    let (db_path_buf, watch_root_buf) = args::parse_args();
    let db_path = db_path_buf.as_path();
    let watch_root = watch_root_buf.as_path();
    info!(db_path = %db_path.display(), watch_root = %watch_root.display(), "Parsed arguments");
    std::io::stdout().flush()?;
    let db = {
        let db_span = info_span!("open_or_create_db");
        let _db_enter = db_span.enter();
        db::open_or_create_db(db_path)?
    };
    info!("Opened/created redb file");
    std::io::stdout().flush()?;
    info!("Ensuring file_cache table exists...");
    std::io::stdout().flush()?;
    db::ensure_file_cache_table(&db)?;
    info!("file_cache table ready");
    std::io::stdout().flush()?;
    let file_cache = Arc::new(Mutex::new(FileCache::with_redb(db)));
    let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
    info!("Created FileCache and Heuristics");
    std::io::stdout().flush()?;
    // Start watcher and cache scan in parallel
    info!("About to start watcher and cache scan in parallel");
    std::io::stdout().flush()?;
    let file_cache_clone = file_cache.clone();
    let heuristics_clone = heuristics;
    let watch_root_buf_clone = watch_root_buf.clone();
    let watcher_handle = std::thread::spawn(move || {
        let watcher_span = info_span!("start_watcher");
        let _watcher_enter = watcher_span.enter();
        watcher::start_watcher(&watch_root_buf_clone, file_cache_clone, heuristics_clone);
        info!("Started watcher");
    });
    let file_cache_bg = file_cache;
    let watch_root_bg = watch_root.to_path_buf();
    let scan_handle = std::thread::spawn(move || {
        if let Ok(mut cache) = file_cache_bg.lock() {
            let load_span = info_span!("load_from_redb");
            let _load_enter = load_span.enter();
            cache.load_from_redb();
            info!("Loaded file cache from redb (background)");
            let scan_span = info_span!("scan_dir");
            let _scan_enter = scan_span.enter();
            cache.scan_dir(&watch_root_bg);
            info!(
                file_count = cache.all_files().count(),
                "After scan_dir (background)"
            );
        } else {
            tracing::error!("failed to lock file_cache for background scan");
        }
    });
    watcher_handle.join().ok();
    scan_handle.join().ok();
    platform::wait_for_exit();
    Ok(())
}
