use notify_debouncer_full::notify::{
    RecursiveMode, Watcher,
    event::{EventKind, ModifyKind},
};
use notify_debouncer_full::{DebouncedEvent, Debouncer, FileIdMap, new_debouncer};
use redb::Builder;
use std::env;
use std::sync::mpsc::Receiver;
use std::time::Duration;

mod move_heuristics;
use move_heuristics::{FileEvent, FileEventKind, MoveHeuristics, make_file_event};
use std::sync::{Arc, Mutex};

mod file_cache;
use file_cache::FileCache;

#[cfg(windows)]
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

// In the rename pairing logic, also print when the parent directory changes (move between folders)
fn start_watcher<P: AsRef<std::path::Path>>(watch_path: P) -> std::io::Result<()> {
    let watch_path = watch_path.as_ref().to_path_buf();
    println!("[Watcher] Watching directory: {:?}", watch_path);
    let (tx, rx) = std::sync::mpsc::channel();
    let mut debouncer = notify_debouncer_full::new_debouncer(Duration::from_millis(500), None, tx)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    debouncer
        .watch(
            &watch_path,
            notify_debouncer_full::notify::RecursiveMode::Recursive,
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let heuristics = Arc::new(Mutex::new(MoveHeuristics::new(Duration::from_secs(5))));
    let heuristics_thread = heuristics.clone();
    let file_cache = Arc::new(Mutex::new(FileCache::new()));
    let file_cache_thread = file_cache.clone();
    // Initial scan
    file_cache.lock().unwrap().scan_dir(&watch_path);

    std::thread::spawn(move || {
        for result in rx {
            match result {
                Ok(events) => {
                    for event in events {
                        // Print event attributes for debugging file ID
                        println!(
                            "[Watcher] Event attributes: tracker={:?}, flag={:?}, info={:?}, source={:?}",
                            event.event.tracker(),
                            event.event.flag(),
                            event.event.info(),
                            event.event.source()
                        );
                        match &event.event.kind {
                            notify_debouncer_full::notify::event::EventKind::Remove(_) => {
                                let path = event.event.paths.get(0).cloned();
                                if let Some(path) = path {
                                    // Use cached metadata for Remove
                                    let meta =
                                        file_cache_thread.lock().unwrap().get(&path).cloned();
                                    let mut file_event =
                                        make_file_event(path.clone(), FileEventKind::Remove);
                                    if let Some(meta) = meta {
                                        file_event.size = Some(meta.size);
                                        file_event.metadata = None; // Optionally, you could store more fields
                                    }
                                    heuristics_thread.lock().unwrap().add_remove(file_event);
                                    file_cache_thread.lock().unwrap().remove_file(&path);
                                }
                                println!("[Watcher] Remove: {:?}", event.event.paths);
                            }
                            notify_debouncer_full::notify::event::EventKind::Create(_) => {
                                let path = event.event.paths.get(0).cloned();
                                if let Some(path) = path {
                                    file_cache_thread.lock().unwrap().update_file(&path);
                                    let file_event =
                                        make_file_event(path.clone(), FileEventKind::Create);
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
                                    // Update cache for rename/move
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Determine directory to watch
    let watch_path = if args.len() > 1 && std::path::Path::new(&args[1]).is_dir() {
        &args[1]
    } else {
        "."
    };
    // Keep watcher alive in main
    start_watcher(watch_path).expect("Failed to start watcher");

    #[cfg(windows)]
    {
        // Register for current user. Set to true for all users (requires admin)
        if let Err(e) = register_redb_extension(false) {
            eprintln!("Failed to register .redb extension: {}", e);
        }
    }

    // Determine database file to open (second argument, must be a file)
    let db_path = if args.len() > 2 && std::path::Path::new(&args[2]).is_file() {
        Some(&args[2])
    } else {
        None
    };

    if let Some(db_path) = db_path {
        println!("Opened via double-click or 'Open with': {}", db_path);
        match Builder::new().open(db_path) {
            Ok(db) => {
                // Try to read the test_table and print the value for 'hello'
                const TABLE: redb::TableDefinition<&str, u64> =
                    redb::TableDefinition::new("test_table");
                let read_txn = db.begin_read().unwrap();
                let table = read_txn.open_table(TABLE);
                match table {
                    Ok(table) => match table.get("hello") {
                        Ok(Some(val)) => println!("Found: ('hello', {})", val.value()),
                        Ok(None) => println!("Key 'hello' not found in 'test_table'."),
                        Err(e) => println!("Error reading from table: {}", e),
                    },
                    Err(e) => println!("Could not open 'test_table': {}", e),
                }
            }
            Err(e) => println!("Failed to open database: {}", e),
        }
    } else {
        // Create a simple test database and insert a value
        let db = Builder::new()
            .create_with_file_format_v3(true)
            .create("test.redb")
            .expect("Failed to create test.redb");
        const TABLE: redb::TableDefinition<&str, u64> = redb::TableDefinition::new("test_table");
        let write_txn = db.begin_write().unwrap();
        {
            let mut table = write_txn.open_table(TABLE).unwrap();
            table.insert("hello", &42).unwrap();
        }
        write_txn.commit().unwrap();
        println!("Created test.redb and inserted ('hello', 42) into 'test_table'.");
    }

    // Keep the console window open if launched by double-click, but do not block the main thread
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
}
