//! Test-only: sequential, streaming, memory-efficient file cache scan and batch commit
// This is not included in production builds.

use linkfield::file_cache::FileCache;
use linkfield::file_cache::db::update_redb_batch_commit;
use linkfield::ignore_config::IgnoreConfig;
use redb::Database;
use std::fs;
use std::path::Path;

/// Sequential, streaming scan: never holds more than `batch_size` file metas in memory.
pub fn scan_dir_sequential_streaming(
	cache: &FileCache,
	db: &Database,
	dir: &Path,
	ignore: &IgnoreConfig,
	parent: Option<u64>,
	batch_size: usize,
	mut on_batch: Option<&mut dyn FnMut(usize)>,
) {
	let parent_key = parent.unwrap_or(cache.root);
	if ignore.is_ignored(dir) {
		return;
	}
	let entries = match fs::read_dir(dir) {
		Ok(e) => e.filter_map(Result::ok).collect::<Vec<_>>(),
		Err(_) => return,
	};
	let mut batch = Vec::with_capacity(batch_size);
	let mut batch_keys = Vec::with_capacity(batch_size);
	let mut batch_count = 0;
	for entry in &entries {
		let path = entry.path();
		if path.is_dir() || ignore.is_ignored(&path) {
			continue;
		}
		let name = match path.file_name().map(|n| n.to_string_lossy()) {
			Some(n) => n.to_string(),
			None => continue,
		};
		if let Some(meta) = linkfield::file_cache::meta::FileMeta::from_path(&path) {
			let key = cache.update_or_insert_file(&name, parent_key, meta.clone());
			batch.push((meta.path.clone(), meta.clone()));
			batch_keys.push(key);
			if batch.len() >= batch_size {
				update_redb_batch_commit(db, &[], &batch);
				for key in &batch_keys {
					cache.entries.remove(key);
				}
				batch.clear();
				batch_keys.clear();
				batch_count += 1;
				if let Some(cb) = on_batch.as_mut() {
					cb(batch_count);
				}
			}
		}
	}
	if !batch.is_empty() {
		update_redb_batch_commit(db, &[], &batch);
		for key in &batch_keys {
			cache.entries.remove(key);
		}
		batch_count += 1;
		if let Some(cb) = on_batch.as_mut() {
			cb(batch_count);
		}
	}
	// Recurse into subdirs sequentially
	for entry in &entries {
		let path = entry.path();
		if !path.is_dir() {
			continue;
		}
		let name = match path.file_name().map(|n| n.to_string_lossy()) {
			Some(n) => n.to_string(),
			None => continue,
		};
		let dir_key = cache.add_dir(&name, parent_key);
		scan_dir_sequential_streaming(
			cache,
			db,
			&path,
			ignore,
			Some(dir_key),
			batch_size,
			None, // Only log batches at the top level
		);
	}
}
