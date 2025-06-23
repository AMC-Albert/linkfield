use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

pub const FILE_CACHE_TABLE: redb::TableDefinition<&str, &[u8]> =
    redb::TableDefinition::new("file_cache");

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct FileMeta {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub size: u64,
    #[allow(dead_code)]
    pub modified: Option<SystemTime>,
    #[allow(dead_code)]
    pub created: Option<SystemTime>,
    #[allow(dead_code)]
    pub extension: Option<String>,
}

impl FileMeta {
    pub fn from_path(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
            created: metadata.created().ok(),
            extension: path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string()),
        })
    }
    pub fn serialize(&self) -> Vec<u8> {
        encode_to_vec(self, bincode::config::standard()).expect("FileMeta serialization failed")
    }
    pub fn deserialize(bytes: &[u8]) -> Self {
        let (meta, _): (Self, _) = decode_from_slice(bytes, bincode::config::standard())
            .expect("FileMeta deserialization failed");
        meta
    }
}

/// A cache of file metadata for all watched files, backed by redb.
pub struct FileCache {
    files: HashMap<PathBuf, FileMeta>,
    #[allow(dead_code)]
    last_scan: Instant,
    db: Option<redb::Database>,
}

impl FileCache {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            last_scan: Instant::now(),
            db: None,
        }
    }
    pub fn with_redb(db: redb::Database) -> Self {
        Self {
            files: HashMap::new(),
            last_scan: Instant::now(),
            db: Some(db),
        }
    }
    /// Scan a directory recursively and return a map of all files (does not update cache or db)
    pub fn scan_dir_collect(dir: &Path) -> HashMap<PathBuf, FileMeta> {
        println!("[FileCache] scan_dir_collect: {:?}", dir);
        std::io::stdout().flush().unwrap();
        let files = Self::collect_files_parallel(dir, 0);
        files.into_iter().collect()
    }

    /// Diff the new scan with the current cache, and atomically update only changes in memory and redb (batched)
    pub fn diff_and_update(&mut self, new_files: HashMap<PathBuf, FileMeta>) {
        let mut added = 0;
        let mut updated = 0;
        let removed = 0;
        let mut unchanged = 0;
        let mut to_remove = Vec::new();
        let mut to_add_or_update = Vec::new();
        // Detect removed files
        for old_path in self.files.keys() {
            if !new_files.contains_key(old_path) {
                to_remove.push(old_path.clone());
            }
        }
        // Detect added/updated files
        for (path, meta) in &new_files {
            match self.files.get(path) {
                Some(old_meta) if old_meta == meta => {
                    unchanged += 1;
                }
                Some(_) => {
                    to_add_or_update.push((path.clone(), meta.clone()));
                    updated += 1;
                }
                None => {
                    to_add_or_update.push((path.clone(), meta.clone()));
                    added += 1;
                }
            }
        }
        // Batch update redb
        if let Some(db) = &self.db {
            let write_txn = db.begin_write().expect("Failed to begin write txn");
            {
                let mut table = write_txn
                    .open_table(FILE_CACHE_TABLE)
                    .expect("Failed to open file_cache table");
                for path in &to_remove {
                    table
                        .remove(path.to_string_lossy().as_ref())
                        .expect("Failed to remove file meta");
                }
                for (path, meta) in &to_add_or_update {
                    table
                        .insert(path.to_string_lossy().as_ref(), meta.serialize().as_slice())
                        .expect("Failed to insert/update file meta");
                }
            }
            write_txn
                .commit()
                .expect("Failed to commit batch diff update");
        }
        // Update in-memory cache to match new_files
        for path in &to_remove {
            self.files.remove(path);
        }
        for (path, meta) in &to_add_or_update {
            self.files.insert(path.clone(), meta.clone());
        }
        println!(
            "[FileCache][diff] Added: {added}, Updated: {updated}, Removed: {removed}, Unchanged: {unchanged}"
        );
        std::io::stdout().flush().unwrap();
    }
    /// Scan a directory and atomically update only changes (calls diff_and_update)
    pub fn scan_dir(&mut self, dir: &Path) {
        let new_files = Self::scan_dir_collect(dir);
        self.diff_and_update(new_files);
    }
    /// Update or insert a file's metadata
    pub fn update_file(&mut self, path: &Path) {
        if let Some(meta) = FileMeta::from_path(path) {
            self.files.insert(path.to_path_buf(), meta.clone());
            if let Some(db) = &self.db {
                let write_txn = db.begin_write().unwrap();
                {
                    let mut table = write_txn.open_table(FILE_CACHE_TABLE).unwrap();
                    table
                        .insert(path.to_string_lossy().as_ref(), meta.serialize().as_slice())
                        .unwrap();
                }
                write_txn.commit().unwrap();
            }
        }
    }
    /// Remove a file from the cache
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(path);
        if let Some(db) = &self.db {
            let write_txn = db.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(FILE_CACHE_TABLE).unwrap();
                table.remove(path.to_string_lossy().as_ref()).unwrap();
            }
            write_txn.commit().unwrap();
        }
    }
    /// Get cached metadata for a file
    pub fn get(&self, path: &Path) -> Option<&FileMeta> {
        self.files.get(path)
    }
    /// Get all cached files
    #[allow(dead_code)]
    pub fn all_files(&self) -> impl Iterator<Item = &FileMeta> {
        self.files.values()
    }
    /// Load the file cache from redb into memory
    pub fn load_from_redb(&mut self) {
        if let Some(db) = &self.db {
            let read_txn = db.begin_read().unwrap();
            let table = read_txn.open_table(FILE_CACHE_TABLE).unwrap();
            self.files.clear();
            for entry in table.range::<&str>(..).unwrap() {
                let (k, v) = entry.unwrap();
                let path = PathBuf::from(k.value());
                let meta = FileMeta::deserialize(v.value());
                self.files.insert(path, meta);
            }
        }
    }
    fn collect_files_parallel(dir: &Path, _depth: usize) -> Vec<(PathBuf, FileMeta)> {
        use rayon::iter::ParallelBridge;
        let entries = match fs::read_dir(dir) {
            Ok(e) => e.par_bridge().filter_map(Result::ok).collect::<Vec<_>>(),
            Err(e) => {
                println!("[FileCache] Error reading dir {:?}: {}", dir, e);
                return Vec::new();
            }
        };
        let (dirs, files): (Vec<_>, Vec<_>) = entries
            .into_par_iter()
            .partition(|entry| entry.path().is_dir());
        let mut results: Vec<(PathBuf, FileMeta)> = files
            .into_par_iter()
            .filter_map(|entry| {
                let path = entry.path();
                FileMeta::from_path(&path).map(|meta| (path, meta))
            })
            .collect();
        let mut sub_results: Vec<(PathBuf, FileMeta)> = dirs
            .into_par_iter()
            .flat_map_iter(|entry| Self::collect_files_parallel(&entry.path(), 0))
            .collect();
        results.append(&mut sub_results);
        results
    }
    #[allow(dead_code)]
    fn add_dir(&mut self, dir: &Path, depth: usize) {
        if depth > 10 {
            println!("[FileCache] Max recursion depth reached at {:?}", dir);
            return;
        }
        println!("[FileCache] add_dir: {:?}", dir);
        std::io::stdout().flush().unwrap();
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            println!("[FileCache] entry: {:?}", path);
                            std::io::stdout().flush().unwrap();
                            if path.is_dir() {
                                self.add_dir(&path, depth + 1);
                            } else if let Some(meta) = FileMeta::from_path(&path) {
                                self.files.insert(path, meta);
                            }
                        }
                        Err(e) => {
                            println!("[FileCache] Error reading entry in {:?}: {}", dir, e);
                        }
                    }
                }
            }
            Err(e) => {
                println!("[FileCache] Error reading dir {:?}: {}", dir, e);
            }
        }
    }
}
