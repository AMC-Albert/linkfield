use redb::Builder;
use std::env;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

mod file_cache;
use file_cache::FileCache;
mod move_heuristics;
use move_heuristics::{FileEventKind, MoveHeuristics, make_file_event};
use std::sync::{Arc, Mutex};

fn start_watcher<P: AsRef<Path>>(
    watch_path: P,
    file_cache: Arc<Mutex<FileCache>>,
    heuristics: Arc<Mutex<MoveHeuristics>>,
) -> std::io::Result<()> {
    let watch_path = watch_path.as_ref().to_path_buf();
    println!("[Watcher] Watching directory: {:?}", watch_path);
    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = notify_debouncer_full::new_debouncer(Duration::from_millis(500), None, tx)
        .map_err(std::io::Error::other)?;
    debouncer
        .watch(
            &watch_path,
            notify_debouncer_full::notify::RecursiveMode::Recursive,
        )
        .map_err(std::io::Error::other)?;

    let heuristics_thread = heuristics.clone();
    let file_cache_thread = file_cache.clone();
    std::thread::spawn(move || {
        println!("[WatcherThread] Event loop started");
        for result in rx {
            match result {
                Ok(events) => {
                    for event in events {
                        println!("[WatcherThread] Received event: {:?}", event);
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
                                println!("[Watcher] Remove: {:?}", event.event.paths);
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
                                    } else {
                                        println!("[Watcher] Create: {:?}", path);
                                    }
                                }
                            }
                            notify_debouncer_full::notify::event::EventKind::Modify(
                                notify_debouncer_full::notify::event::ModifyKind::Name(_),
                            ) => {
                                let paths = &event.event.paths;
                                if paths.len() == 2 {
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
                                } else {
                                    println!(
                                        "[Watcher] Rename/Move event with unexpected paths: {:?}",
                                        paths
                                    );
                                }
                            }
                            _ => {
                                println!("[Watcher] Event: {:?}", event);
                            }
                        }
                    }
                }
                Err(e) => println!("[Watcher] Error: {:?}", e),
            }
        }
    });
    println!("[Watcher] Ready. Try renaming, creating, or deleting files in this directory.");
    Box::leak(Box::new(debouncer));
    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
fn register_redb_extension(all_users: bool) -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::*;
    let exe_path = env::current_exe()?.to_str().unwrap().to_string();
    let prog_id = "Linkfield.redb";
    let friendly_name = "Linkfield Database File";
    let root = if all_users {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    };
    // Set .redb default value to our ProgID
    let (redb_key, _) = root.create_subkey(r"Software\Classes\.redb")?;
    redb_key.set_value("", &prog_id)?;
    // Register ProgID
    let (progid_key, _) = root.create_subkey(format!(r"Software\\Classes\\{}", prog_id))?;
    progid_key.set_value("", &friendly_name)?;
    let (shell_key, _) = progid_key.create_subkey(r"shell\open\command")?;
    shell_key.set_value("", &format!("\"{}\" \"%1\"", exe_path))?;
    // Set the icon for .redb files to the program's own icon
    let (default_icon_key, _) = progid_key.create_subkey("DefaultIcon")?;
    default_icon_key.set_value("", &format!("\"{}\",0", exe_path))?;
    println!(".redb extension registered to {}", exe_path);
    Ok(())
}

fn main() {
    println!("[main] Starting linkfield");
    std::io::stdout().flush().unwrap();
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
    std::io::stdout().flush().unwrap();
    let db = if db_path.exists() {
        Builder::new()
            .create_with_file_format_v3(true)
            .open(db_path)
            .expect("Failed to open redb file")
    } else {
        Builder::new()
            .create_with_file_format_v3(true)
            .create(db_path)
            .expect("Failed to create redb file")
    };
    println!("[main] Opened/created redb file");
    std::io::stdout().flush().unwrap();
    println!("[main] Ensuring file_cache table exists...");
    std::io::stdout().flush().unwrap();
    // Ensure the file_cache table exists (open_table creates if missing)
    {
        let write_txn = db.begin_write().expect("Failed to begin write txn");
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
    std::io::stdout().flush().unwrap();
    let file_cache = Arc::new(Mutex::new(FileCache::with_redb(db)));
    let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
    println!("[main] Created FileCache and Heuristics");
    std::io::stdout().flush().unwrap();
    // Load cache from redb BEFORE first scan_dir
    file_cache.lock().unwrap().load_from_redb();
    println!("[main] Loaded file cache from redb");
    // File cache scan/logging BEFORE watcher
    println!("[main] About to scan_dir");
    std::io::stdout().flush().unwrap();
    file_cache.lock().unwrap().scan_dir(watch_root);
    println!(
        "[FileCache] After scan_dir: {} files",
        file_cache.lock().unwrap().all_files().count()
    );
    std::io::stdout().flush().unwrap();
    println!("[main] About to start watcher");
    std::io::stdout().flush().unwrap();
    start_watcher(watch_root, file_cache.clone(), heuristics).expect("Failed to start watcher");
    println!("[main] Started watcher");
    std::io::stdout().flush().unwrap();

    // Print file cache stats after initial scan
    {
        let cache = file_cache.lock().unwrap();
        let count = cache.all_files().count();
        let total_size: u64 = cache.all_files().map(|m| m.size).sum();
        println!(
            "[FileCache] Initial files cached: {} (total size: {} bytes)",
            count, total_size
        );
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
            if handle.read(&mut buf).is_ok() && buf[0] == b'\n' {
                break;
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
            if handle.read(&mut buf).is_ok() && buf[0] == b'\n' {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
