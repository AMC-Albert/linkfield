//! Integration test: file cache is committed to redb in batches, not kept fully in memory

use linkfield::file_cache::FileCache;
use linkfield::file_cache::db::{FILE_CACHE_TABLE, ensure_file_cache_table};
use redb::{Database, ReadableTableMetadata};
use std::fs::{self, File};
use std::io::Write;
use sysinfo::{ProcessesToUpdate, System};
use tempfile::tempdir;

#[test]
fn test_file_cache_batch_commit() {
	let mut sys = System::new_all();
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let pid = sysinfo::get_current_pid().unwrap();
	let process = sys.process(pid).unwrap();
	let mem_before = process.memory();
	println!("Memory before file creation: {} KB", mem_before);

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

	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_files = sys.process(pid).unwrap().memory();
	println!("Memory after file creation: {} KB", mem_after_files);

	// Scan and commit in batches
	let cache = FileCache::new_root("files");
	let ignore = linkfield::ignore_config::IgnoreConfig::empty();
	let mut batch_logger = |batch_num: usize| {
		sys.refresh_processes(ProcessesToUpdate::All, true);
		let mem = sys.process(pid).unwrap().memory();
		println!("[test] After batch {batch_num}: memory = {mem} KB");
	};
	cache.scan_dir_collect_with_ignore_and_commit(
		&db,
		&dir,
		&ignore,
		None,
		1000,
		Some(&mut batch_logger),
	);
	// Give allocator/OS a chance to reclaim memory
	std::thread::sleep(std::time::Duration::from_secs(1));
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_scan = sys.process(pid).unwrap().memory();
	println!("Memory after scan: {} KB", mem_after_scan);
	println!("Cache entry count after scan: {}", cache.entries.len());
	// Drop the in-memory cache to free memory
	drop(cache);
	std::thread::sleep(std::time::Duration::from_secs(1));
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_drop = sys.process(pid).unwrap().memory();
	println!("Memory after drop: {} KB", mem_after_drop);

	// Open a new transaction and check that all files are in the db
	let txn = db.begin_read().unwrap();
	let table = txn.open_table(FILE_CACHE_TABLE).unwrap();
	let len = ReadableTableMetadata::len(&table).unwrap();
	assert_eq!(len, 5000);

	// Assert that memory usage does not grow linearly with file count
	// Allow some leeway for OS/disk cache, but memory after scan should not be much higher than after file creation
	assert!(
		mem_after_scan < mem_after_files + 20_000,
		"Memory usage grew too much during scan: before files = {mem_before}, after files = {mem_after_files}, after scan = {mem_after_scan}"
	);
	assert!(
		mem_after_drop <= mem_after_scan,
		"Memory was not released after dropping cache"
	);
}

#[test]
fn test_file_cache_sequential_streaming() {
	let mut sys = System::new_all();
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let pid = sysinfo::get_current_pid().unwrap();
	let process = sys.process(pid).unwrap();
	let mem_before = process.memory();
	println!(
		"[sequential] Memory before file creation: {} KB",
		mem_before
	);

	let temp = tempdir().unwrap();
	let db_path = temp.path().join("test_seq.redb");
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

	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_files = sys.process(pid).unwrap().memory();
	println!(
		"[sequential] Memory after file creation: {} KB",
		mem_after_files
	);

	// Scan and commit in batches, sequential streaming
	let cache = FileCache::new_root("files");
	let ignore = linkfield::ignore_config::IgnoreConfig::empty();
	let mut batch_logger = |batch_num: usize| {
		sys.refresh_processes(ProcessesToUpdate::All, true);
		let mem = sys.process(pid).unwrap().memory();
		println!("[sequential] After batch {batch_num}: memory = {mem} KB");
	};
	sequential_scan::scan_dir_sequential_streaming(
		&cache,
		&db,
		&dir,
		&ignore,
		None,
		1000,
		Some(&mut batch_logger),
	);
	std::thread::sleep(std::time::Duration::from_secs(1));
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_scan = sys.process(pid).unwrap().memory();
	println!("[sequential] Memory after scan: {} KB", mem_after_scan);
	println!(
		"[sequential] Cache entry count after scan: {}",
		cache.entries.len()
	);
	drop(cache);
	std::thread::sleep(std::time::Duration::from_secs(1));
	sys.refresh_processes(ProcessesToUpdate::All, true);
	let mem_after_drop = sys.process(pid).unwrap().memory();
	println!("[sequential] Memory after drop: {} KB", mem_after_drop);

	let txn = db.begin_read().unwrap();
	let table = txn.open_table(FILE_CACHE_TABLE).unwrap();
	let len = ReadableTableMetadata::len(&table).unwrap();
	assert_eq!(len, 5000);

	assert!(
		mem_after_scan < mem_after_files + 20_000,
		"[sequential] Memory usage grew too much during scan: before files = {mem_before}, after files = {mem_after_files}, after scan = {mem_after_scan}"
	);
	assert!(
		mem_after_drop <= mem_after_scan,
		"[sequential] Memory was not released after dropping cache"
	);
}

#[path = "sequential_scan.rs"]
mod sequential_scan;
