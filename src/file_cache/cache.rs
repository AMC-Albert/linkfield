//! `FileCache`: in-memory and persistent file metadata cache

use slotmap::{SlotMap, new_key_type};

new_key_type! { pub struct EntryKey; }

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
	pub parent: Option<EntryKey>,
	pub kind: EntryKind,
}

/// `FileCache`: stores file and directory metadata in a tree using slotmap keys
pub struct FileCache {
	pub entries: SlotMap<EntryKey, DirEntry>,
	pub root: EntryKey,
}

impl FileCache {
	/// Create a new file cache with a root directory
	pub fn new_root(root_name: &str) -> Self {
		let mut entries = SlotMap::with_key();
		let root = entries.insert(DirEntry {
			name: root_name.to_string(),
			parent: None,
			kind: EntryKind::Directory,
		});
		Self { entries, root }
	}
	/// Add a directory under a parent
	pub fn add_dir(&mut self, name: &str, parent: EntryKey) -> EntryKey {
		self.entries.insert(DirEntry {
			name: name.to_string(),
			parent: Some(parent),
			kind: EntryKind::Directory,
		})
	}
	/// Add or update a file under a parent directory
	pub fn update_or_insert_file(
		&mut self,
		name: &str,
		parent: EntryKey,
		meta: crate::file_cache::meta::FileMeta,
	) -> EntryKey {
		if let Some(existing) = self.find_child_by_name(parent, name) {
			if let Some(entry) = self.entries.get_mut(existing) {
				entry.kind = EntryKind::File(meta);
			}
			existing
		} else {
			self.entries.insert(DirEntry {
				name: name.to_string(),
				parent: Some(parent),
				kind: EntryKind::File(meta),
			})
		}
	}
	/// Remove an entry and all its descendants
	pub fn remove_entry(&mut self, key: EntryKey) {
		let children: Vec<_> = self
			.entries
			.iter()
			.filter(|(_, entry)| entry.parent == Some(key))
			.map(|(k, _)| k)
			.collect();
		for child in children {
			self.remove_entry(child);
		}
		self.entries.remove(key);
	}
	/// Find a child entry by name under a parent
	pub fn find_child_by_name(&self, parent: EntryKey, name: &str) -> Option<EntryKey> {
		self.entries
			.iter()
			.find(|(_, entry)| entry.parent == Some(parent) && entry.name == name)
			.map(|(k, _)| k)
	}
	#[allow(dead_code)]
	/// Reconstruct the full path for an entry
	pub fn reconstruct_path(&self, mut id: EntryKey) -> std::path::PathBuf {
		let mut components = Vec::new();
		while let Some(entry) = self.entries.get(id) {
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
	pub fn find_entry_by_path<P: AsRef<std::path::Path>>(&self, path: P) -> Option<EntryKey> {
		let mut current = self.root;
		let mut components = path.as_ref().components().peekable();
		// Skip root if it matches
		if let Some(root_entry) = self.entries.get(self.root) {
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
	/// Get file metadata by path
	pub fn get(&self, path: &std::path::Path) -> Option<&crate::file_cache::meta::FileMeta> {
		let key = self.find_entry_by_path(path)?;
		match &self.entries.get(key)?.kind {
			EntryKind::File(meta) => Some(meta),
			_ => None,
		}
	}
	/// Remove a file or directory by path
	pub fn remove_file(&mut self, path: &std::path::Path) {
		if let Some(key) = self.find_entry_by_path(path) {
			self.remove_entry(key);
		}
	}
	/// Update or insert a file by path
	pub fn update_file(&mut self, path: &std::path::Path) {
		if let Some(meta) = crate::file_cache::meta::FileMeta::from_path(path) {
			let mut current = self.root;
			let components: Vec<_> = path.components().collect();
			let mut idx = 0;
			// Skip root if it matches
			if let Some(root_entry) = self.entries.get(self.root) {
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
		&mut self,
		dir: &std::path::Path,
		ignore: &linkfield::ignore::IgnoreConfig,
		parent: Option<EntryKey>,
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
		for (path, name) in subdirs {
			let dir_key = self.add_dir(&name, parent_key);
			self.scan_dir_collect_with_ignore(&path, ignore, Some(dir_key));
		}
	}
	/// Return all file metas in the tree
	pub fn all_files(&self) -> impl Iterator<Item = &crate::file_cache::meta::FileMeta> {
		self.entries.values().filter_map(|entry| match &entry.kind {
			EntryKind::File(meta) => Some(meta),
			_ => None,
		})
	}
}
