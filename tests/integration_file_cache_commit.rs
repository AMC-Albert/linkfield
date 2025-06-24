//! Integration test: file cache is committed to redb in batches, not kept fully in memory

use linkfield::file_cache::db::{
	FILE_CACHE_TABLE, ensure_file_cache_table,
};
use linkfield::file_cache::FileCache;
use redb::{Database, ReadableTableMetadata};
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_file_cache_batch_commit() {
	let temp = tempdir().unwrap();
	let db_path = temp.path().join("test.redb");
	let db = Database::create(&db_path).unwrap();
	ensure_file_cache_table(&db).unwrap();

	// Create a directory with many files
	let dir = temp.path().join("files");
	fs::create_dir(&dir).unwrap();
	for i in 0..5000 {
		let file_path = dir.join(format!("file_{i}.txt"));
		let mut f = File::create(&file_path).unwrap();
		writeln!(f, "hello {i}").unwrap();
	}

	// Scan and commit in batches
	let cache = FileCache::new_root("files");
	let ignore = linkfield::ignore_config::IgnoreConfig::empty();
	cache.scan_dir_collect_with_ignore_and_commit(&db, &dir, &ignore, None, 1000);

	// Drop the in-memory cache to free memory
	drop(cache);

	// Open a new transaction and check that all files are in the db
	let txn = db.begin_read().unwrap();
	let table = txn.open_table(FILE_CACHE_TABLE).unwrap();
	let len = ReadableTableMetadata::len(&table).unwrap();
	assert_eq!(len, 5000);
}
