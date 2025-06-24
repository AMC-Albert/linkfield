use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use linkfield::args;
use linkfield::db;
use linkfield::file_cache::FileCache;
use linkfield::ignore_config::IgnoreConfig;
use linkfield::move_heuristics::MoveHeuristics;
use linkfield::platform;
use linkfield::watcher;
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
	let mut db = {
		let db_span = info_span!("open_or_create_db");
		let _db_enter = db_span.enter();
		db::open_or_create_db(db_path)?
	};
	info!("Opened/created redb file");
	std::io::stdout().flush()?;
	info!("Ensuring file_cache table exists...");
	std::io::stdout().flush()?;
	linkfield::file_cache::ensure_file_cache_table(&db)?;
	info!("file_cache table ready");
	std::io::stdout().flush()?;
	// Use FileCache::new_root with the root dir name
	let file_cache = FileCache::new_root(watch_root.to_string_lossy().as_ref());
	let file_cache = Arc::new(Mutex::new(file_cache));
	let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
	info!("Created FileCache and Heuristics");
	std::io::stdout().flush()?;
	// Load ignore config from .linkfieldignore and log patterns
	let (ignore_config, _ignore_patterns) =
		match IgnoreConfig::from_file_with_patterns(".linkfieldignore") {
			Ok((cfg, pats)) => {
				info!(ignore_patterns = ?pats, "Loaded ignore patterns from .linkfieldignore");
				(cfg, pats)
			}
			Err(e) => {
				tracing::warn!(error = %e, "Failed to load .linkfieldignore, ignoring patterns");
				(IgnoreConfig::empty(), vec![])
			}
		};
	let ignore_config = Arc::new(ignore_config);
	// Start watcher and cache scan in parallel
	info!("About to start watcher and cache scan in parallel");
	std::io::stdout().flush()?;
	let file_cache_clone = file_cache.clone();
	let heuristics_clone = heuristics;
	let watch_root_buf_clone = watch_root_buf.clone();
	let ignore_config_clone = ignore_config.clone();
	let watcher_handle = std::thread::spawn(move || {
		let watcher_span = info_span!("start_watcher");
		let _watcher_enter = watcher_span.enter();
		watcher::start_watcher(
			&watch_root_buf_clone,
			file_cache_clone,
			heuristics_clone,
			ignore_config_clone,
		);
		info!("Started watcher");
	});
	let file_cache_bg = file_cache;
	let watch_root_bg = watch_root.to_path_buf();
	let ignore_config_bg = ignore_config;
	let scan_handle = std::thread::spawn(move || {
		if let Ok(cache) = file_cache_bg.lock() {
			let scan_span = info_span!("scan_dir");
			let _scan_enter = scan_span.enter();
			cache.scan_dir_collect_with_ignore_and_commit(
				&db,
				&watch_root_bg,
				&ignore_config_bg,
				None,
				1000,
			);
			info!(
				file_count = cache.all_files().len(),
				"After scan_dir (background)"
			);
			// Optionally compact the database after scan
			match db::compact_database(&mut db) {
				Ok(true) => info!("Database compaction performed"),
				Ok(false) => info!("Database compaction not needed"),
				Err(e) => tracing::warn!(error = %e, "Database compaction failed"),
			}
		} else {
			tracing::error!("failed to lock file_cache for background scan");
		}
	});
	watcher_handle.join().ok();
	scan_handle.join().ok();
	platform::wait_for_exit();
	Ok(())
}
