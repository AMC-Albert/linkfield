// Database setup and table creation logic will be moved here

use redb::{Builder, Database};
use std::error::Error;
use std::path::Path;

pub fn open_or_create_db(db_path: &Path) -> Result<Database, Box<dyn Error>> {
    let db = if db_path.exists() {
        Builder::new()
            .create_with_file_format_v3(true)
            .open(db_path)
            .map_err(|e| {
                tracing::error!(error = %e, path = %db_path.display(), "Failed to open redb file");
                e
            })?
    } else {
        Builder::new()
            .create_with_file_format_v3(true)
            .create(db_path)
            .map_err(|e| {
                tracing::error!(error = %e, path = %db_path.display(), "Failed to create redb file");
                e
            })?
    };
    Ok(db)
}

pub fn ensure_file_cache_table(db: &Database) -> Result<(), Box<dyn Error>> {
    let write_txn = match db.begin_write() {
        Ok(txn) => txn,
        Err(e) => {
            tracing::error!(error = %e, "Failed to begin write txn");
            return Err(Box::new(e));
        }
    };
    match write_txn.open_table(crate::file_cache::FILE_CACHE_TABLE) {
        Ok(_) => tracing::info!("file_cache table opened/created successfully"),
        Err(e) => {
            tracing::error!(error = %e, "Failed to open/create file_cache table");
            std::process::exit(1);
        }
    }
    if let Err(e) = write_txn.commit() {
        tracing::error!(error = %e, "Failed to commit table creation");
        std::process::exit(1);
    }
    Ok(())
}
