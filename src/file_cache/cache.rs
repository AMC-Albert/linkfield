//! `FileCache`: in-memory and persistent file metadata cache

use crate::ignore_config::IgnoreConfig;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub enum EntryKind {
	File(crate::file_cache::meta::FileMeta),
	Directory,
}

impl PartialEq for EntryKind {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Directory, Self::Directory) => true,
			(Self::File(a), Self::File(b)) => a == b,
			_ => false,
		}
	}
}

#[derive(Debug, Clone)]
pub struct DirEntry {
	pub name: String,
	pub parent: Option<u64>,
	pub kind: EntryKind,
}

/// `FileCache`: stores file and directory metadata in a tree using slotmap keys
pub struct FileCache {
	pub entries: DashMap<u64, DirEntry>,
	pub root: u64,
	key_counter: AtomicU64,
}

impl FileCache {
	/// Create a new file cache with a root directory
	pub fn new_root(root_name: &str) -> std::sync::Arc<Self> {
		let entries = DashMap::new();
		let key_counter = AtomicU64::new(2); // Start at 2, root is 1
		let root_key = 1u64;
		entries.insert(
			root_key,
			DirEntry {
				name: root_name.to_string(),
				parent: None,
				kind: EntryKind::Directory,
			},
		);
		std::sync::Arc::new(Self {
			entries,
			root: root_key,
			key_counter,
		})
	}
	fn next_key(&self) -> u64 {
		self.key_counter.fetch_add(1, Ordering::Relaxed)
	}
	/// Add a directory under a parent
	pub fn add_dir(&self, name: &str, parent: u64) -> u64 {
		let key = self.next_key();
		self.entries.insert(
			key,
			DirEntry {
				name: name.to_string(),
				parent: Some(parent),
				kind: EntryKind::Directory,
			},
		);
		key
	}
	/// Add or update a file under a parent directory
	pub fn update_or_insert_file(
		&self,
		name: &str,
		parent: u64,
		meta: crate::file_cache::meta::FileMeta,
	) -> u64 {
		if let Some(existing) = self.find_child_by_name(parent, name) {
			if let Some(mut entry) = self.entries.get_mut(&existing) {
				entry.kind = EntryKind::File(meta);
			}
			existing
		} else {
			let key = self.next_key();
			self.entries.insert(
				key,
				DirEntry {
					name: name.to_string(),
					parent: Some(parent),
					kind: EntryKind::File(meta),
				},
			);
			key
		}
	}
	/// Remove an entry and all its descendants
	pub fn remove_entry(&self, key: u64) {
		let children: Vec<_> = self
			.entries
			.iter()
			.filter(|entry| entry.parent == Some(key))
			.map(|entry| *entry.key())
			.collect();
		for child in children {
			self.remove_entry(child);
		}
		self.entries.remove(&key);
	}
	/// Find a child entry by name under a parent
	pub fn find_child_by_name(&self, parent: u64, name: &str) -> Option<u64> {
		self.entries
			.iter()
			.find(|entry| entry.parent == Some(parent) && entry.name == name)
			.map(|entry| *entry.key())
	}
	#[allow(dead_code)]
	/// Reconstruct the full path for an entry
	pub fn reconstruct_path(&self, mut id: u64) -> std::path::PathBuf {
		let mut components = Vec::new();
		while let Some(entry) = self.entries.get(&id) {
			components.push(entry.name.clone());
			if let Some(parent) = entry.parent {
				id = parent;
			} else {
				break;
			}
		}
		components.reverse();
		components.iter().collect()
	}
	/// Find an entry by absolute path, starting from root
	pub fn find_entry_by_path<P: AsRef<std::path::Path>>(&self, path: P) -> Option<u64> {
		let mut current = self.root;
		let mut components = path.as_ref().components().peekable();
		// Skip root if it matches
		if let Some(root_entry) = self.entries.get(&self.root) {
			if let Some(first) = components.peek() {
				if first.as_os_str().to_string_lossy() == root_entry.name {
					components.next();
				}
			}
		}
		for comp in components {
			let name = comp.as_os_str().to_string_lossy();
			if let Some(child) = self.find_child_by_name(current, &name) {
				current = child;
			} else {
				return None;
			}
		}
		Some(current)
	}
	/// Get file metadata by path (returns owned FileMeta)
	pub fn get(&self, path: &std::path::Path) -> Option<crate::file_cache::meta::FileMeta> {
		let key = self.find_entry_by_path(path)?;
		match self.entries.get(&key)?.kind {
			EntryKind::File(ref meta) => Some(meta.clone()),
			_ => None,
		}
	}
	/// Remove a file or directory by path
	pub fn remove_file(&self, path: &std::path::Path) {
		if let Some(key) = self.find_entry_by_path(path) {
			self.remove_entry(key);
		}
	}
	/// Update or insert a file by path
	pub fn update_file(&self, path: &std::path::Path) {
		if let Some(meta) = crate::file_cache::meta::FileMeta::from_path(path) {
			let mut current = self.root;
			let components: Vec<_> = path.components().collect();
			let mut idx = 0;
			// Skip root if it matches
			if let Some(root_entry) = self.entries.get(&self.root) {
				if !components.is_empty()
					&& components[0].as_os_str().to_string_lossy() == root_entry.name
				{
					idx += 1;
				}
			}
			for (i, comp) in components[idx..].iter().enumerate() {
				let name = comp.as_os_str().to_string_lossy();
				if i < components.len() - idx - 1 {
					// Directory
					if let Some(child) = self.find_child_by_name(current, &name) {
						current = child;
					} else {
						current = self.add_dir(&name, current);
					}
				} else {
					// Last component is file
					self.update_or_insert_file(&name, current, meta.clone());
				}
			}
		}
	}
	/// Recursively scan a directory and populate the tree, respecting ignore rules, using Rayon for parallelism
	pub fn scan_dir_collect_with_ignore(
		&self,
		dir: &std::path::Path,
		ignore: &IgnoreConfig,
		parent: Option<u64>,
	) {
		use rayon::prelude::*;
		use std::fs;
		let parent_key = parent.unwrap_or(self.root);
		if ignore.is_ignored(dir) {
			tracing::info!(ignore_match = %dir.display(), "ignoring directory due to ignore config");
			return;
		}
		let entries = match fs::read_dir(dir) {
			Ok(e) => e.filter_map(Result::ok).collect::<Vec<_>>(),
			Err(e) => {
				tracing::warn!(error = %e, dir = %dir.display(), "Error reading dir");
				return;
			}
		};
		// Collect file metas in parallel
		let file_metas: Vec<_> = entries
			.par_iter()
			.filter_map(|entry| {
				let path = entry.path();
				if path.is_dir() || ignore.is_ignored(&path) {
					return None;
				}
				let name = path.file_name().map(|n| n.to_string_lossy())?;
				let meta = crate::file_cache::meta::FileMeta::from_path(&path)?;
				Some((name.to_string(), meta))
			})
			.collect();
		for (name, meta) in file_metas {
			self.update_or_insert_file(&name, parent_key, meta);
		}
		// Collect subdirs in parallel
		let subdirs: Vec<_> = entries
			.par_iter()
			.filter_map(|entry| {
				let path = entry.path();
				if !path.is_dir() {
					return None;
				}
				let name = path.file_name().map(|n| n.to_string_lossy())?;
				Some((path.clone(), name.to_string()))
			})
			.collect();
		for (_path, _name) in subdirs {
			let _dir_key = self.add_dir(&_name, parent_key);
			// self.scan_dir_collect_with_ignore_and_commit(&path, ignore, Some(dir_key));
		}
	}
	/// Parallel recursive scan and commit using Rayon. Thread-safe, full parallelism.
	pub fn scan_dir_collect_with_ignore_and_commit(
		self: &std::sync::Arc<Self>,
		db: &redb::Database,
		dir: &std::path::Path,
		ignore: &IgnoreConfig,
		parent: Option<u64>,
		batch_size: usize,
		mut on_batch: Option<&mut dyn FnMut(usize)>,
	) {
		use rayon::prelude::*;
		use std::fs;
		let parent_key = parent.unwrap_or(self.root);
		if ignore.is_ignored(dir) {
			tracing::info!(ignore_match = %dir.display(), "ignoring directory due to ignore config");
			return;
		}
		let entries = match fs::read_dir(dir) {
			Ok(e) => e.filter_map(Result::ok).collect::<Vec<_>>(),
			Err(e) => {
				tracing::warn!(error = %e, dir = %dir.display(), "Error reading dir");
				return;
			}
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
			if let Some(meta) = crate::file_cache::meta::FileMeta::from_path(&path) {
				let key = self.update_or_insert_file(&name, parent_key, meta.clone());
				batch.push((meta.path.clone(), meta.clone()));
				batch_keys.push(key);
				if batch.len() >= batch_size {
					crate::file_cache::db::update_redb_batch_commit(db, &[], &batch);
					for key in &batch_keys {
						self.entries.remove(key);
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
			crate::file_cache::db::update_redb_batch_commit(db, &[], &batch);
			for key in &batch_keys {
				self.entries.remove(key);
			}
			batch_count += 1;
			if let Some(cb) = on_batch.as_mut() {
				cb(batch_count);
			}
		}
		// Collect subdirs and recurse in parallel
		let subdirs: Vec<_> = entries
			.iter()
			.filter_map(|entry| {
				let path = entry.path();
				if !path.is_dir() {
					return None;
				}
				let name = path.file_name().map(|n| n.to_string_lossy())?;
				Some((path.clone(), name.to_string()))
			})
			.collect();
		subdirs.par_iter().for_each(|(path, name)| {
			let dir_key = self.add_dir(name, parent_key);
			self.clone().scan_dir_collect_with_ignore_and_commit(
				db,
				path,
				ignore,
				Some(dir_key),
				batch_size,
				None, // Don't propagate callback to subdirs for simplicity
			);
		});
	}
	/// Return all file metas in the tree
	pub fn all_files(&self) -> Vec<crate::file_cache::meta::FileMeta> {
		self.entries
			.iter()
			.filter_map(|entry| match &entry.kind {
				EntryKind::File(meta) => Some(meta.clone()),
				_ => None,
			})
			.collect()
	}
}
