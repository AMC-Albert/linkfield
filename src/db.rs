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

/// Compact the redb database file, returning true if compaction was performed
pub fn compact_database(db: &mut Database) -> Result<bool, redb::CompactionError> {
	db.compact()
}
