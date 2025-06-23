// Database setup and table creation logic will be moved here

use redb::{Builder, Database};
use std::path::Path;

pub fn open_or_create_db(db_path: &Path) -> Result<Database, Box<dyn std::error::Error>> {
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
    Ok(db)
}

pub fn ensure_file_cache_table(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    let write_txn = match db.begin_write() {
        Ok(txn) => txn,
        Err(e) => {
            eprintln!("[main] ERROR: failed to begin write txn: {e}");
            return Err(Box::new(e));
        }
    };
    match write_txn.open_table(crate::file_cache::FILE_CACHE_TABLE) {
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
    Ok(())
}
