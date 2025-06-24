//! redb helpers for file cache
use crate::file_cache::meta::{FileCachePath, FileMeta};
use tracing::debug;

pub const FILE_CACHE_TABLE: redb::TableDefinition<&str, &[u8]> =
	redb::TableDefinition::new("file_cache");

/// Ensure the `file_cache` table exists in the database
pub fn ensure_file_cache_table(db: &redb::Database) -> Result<(), Box<dyn std::error::Error>> {
	let write_txn = match db.begin_write() {
		Ok(txn) => txn,
		Err(e) => {
			tracing::error!(error = %e, "Failed to begin write txn");
			return Err(Box::new(e));
		}
	};
	match write_txn.open_table(FILE_CACHE_TABLE) {
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

impl FileMeta {
	pub fn key_str(&self) -> String {
		self.path.0.to_string_lossy().to_string()
	}
}

// Return &str instead of String for redb
pub fn serialize_path(path: &FileCachePath) -> std::borrow::Cow<'_, str> {
	path.0.to_string_lossy()
}

pub fn update_redb_batch_commit(
	db: &redb::Database,
	to_remove: &[FileCachePath],
	to_add_or_update: &[(FileCachePath, FileMeta)],
) {
	debug!(
		"Committing batch of {} files, removing {}",
		to_add_or_update.len(),
		to_remove.len()
	);
	let write_txn = match db.begin_write() {
		Ok(txn) => txn,
		Err(e) => {
			tracing::error!(error = %e, "Failed to begin write txn");
			return;
		}
	};
	let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
		Ok(t) => t,
		Err(e) => {
			tracing::error!(error = %e, "Failed to open file_cache table");
			return;
		}
	};
	for path in to_remove {
		if let Err(e) = table.remove(serialize_path(path).as_ref()) {
			tracing::error!(error = %e, path = %path.0.display(), "Failed to remove file meta");
		}
	}
	for (path, meta) in to_add_or_update {
		if let Err(e) = table.insert(serialize_path(path).as_ref(), meta.serialize().as_slice()) {
			tracing::error!(error = %e, path = %path.0.display(), "Failed to insert/update file meta");
		}
	}
	drop(table);
	if let Err(e) = write_txn.commit() {
		tracing::error!(error = %e, "Failed to commit batch diff update");
	}
}

pub fn update_redb_single_insert(db: &redb::Database, path: &FileCachePath, meta: &FileMeta) {
	let write_txn = match db.begin_write() {
		Ok(txn) => txn,
		Err(e) => {
			tracing::error!(error = %e, "Failed to begin write txn");
			return;
		}
	};
	let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
		Ok(t) => t,
		Err(e) => {
			tracing::error!(error = %e, "Failed to open file_cache table");
			return;
		}
	};
	if let Err(e) = table.insert(serialize_path(path).as_ref(), meta.serialize().as_slice()) {
		tracing::error!(error = %e, path = %path.0.display(), "Failed to insert/update file meta");
	}
	drop(table);
	if let Err(e) = write_txn.commit() {
		tracing::error!(error = %e, "Failed to commit update");
	}
}

pub fn update_redb_single_remove(db: &redb::Database, path: &FileCachePath) {
	let write_txn = match db.begin_write() {
		Ok(txn) => txn,
		Err(e) => {
			tracing::error!(error = %e, "Failed to begin write txn");
			return;
		}
	};
	let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
		Ok(t) => t,
		Err(e) => {
			tracing::error!(error = %e, "Failed to open file_cache table");
			return;
		}
	};
	if let Err(e) = table.remove(serialize_path(path).as_ref()) {
		tracing::error!(error = %e, path = %path.0.display(), "Failed to remove file meta");
	}
	drop(table);
	if let Err(e) = write_txn.commit() {
		tracing::error!(error = %e, "Failed to commit remove");
	}
}
