use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
                .map(std::string::ToString::to_string),
        })
    }
    pub fn serialize(&self) -> Vec<u8> {
        encode_to_vec(self, bincode::config::standard()).unwrap_or_else(|e| {
            eprintln!("[FileMeta] Serialization failed: {e}");
            Vec::new()
        })
    }
    pub fn deserialize(bytes: &[u8]) -> Self {
        let (meta, _) = decode_from_slice(bytes, bincode::config::standard()).unwrap_or_else(|e| {
            eprintln!("[FileMeta] Deserialization failed: {e}");
            (
                Self {
                    path: PathBuf::new(),
                    size: 0,
                    modified: None,
                    created: None,
                    extension: None,
                },
                0,
            )
        });
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
        println!("[FileCache] scan_dir_collect: {}", dir.display());
        if let Err(e) = std::io::stdout().flush() {
            eprintln!("[FileCache] Failed to flush stdout: {e}");
        }
        let counter = Arc::new(AtomicUsize::new(0));
        let files = Self::collect_files_parallel_progress(dir, 0, &counter);
        // Print final progress
        print!(
            "\r[FileCache] Scanned {} files.\n",
            counter.load(Ordering::Relaxed)
        );
        if let Err(e) = std::io::stdout().flush() {
            eprintln!("[FileCache] Failed to flush stdout: {e}");
        }
        files.into_iter().collect()
    }
    fn collect_files_parallel_progress(
        dir: &Path,
        _depth: usize,
        counter: &Arc<AtomicUsize>,
    ) -> Vec<(PathBuf, FileMeta)> {
        use rayon::iter::ParallelBridge;
        let entries = match fs::read_dir(dir) {
            Ok(e) => e.par_bridge().filter_map(Result::ok).collect::<Vec<_>>(),
            Err(e) => {
                println!("[FileCache] Error reading dir {}: {e}", dir.display());
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
                let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 100 == 0 {
                    print!("\r[FileCache] Scanning... {count} files");
                    if let Err(e) = std::io::stdout().flush() {
                        eprintln!("[FileCache] Failed to flush stdout: {e}");
                    }
                }
                FileMeta::from_path(&path).map(|meta| (path, meta))
            })
            .collect();
        let mut sub_results: Vec<(PathBuf, FileMeta)> = dirs
            .into_par_iter()
            .flat_map_iter(|entry| Self::collect_files_parallel_progress(&entry.path(), 0, counter))
            .collect();
        results.append(&mut sub_results);
        results
    }
    /// Diff the new scan with the current cache, and atomically update only changes in memory and redb (batched)
    pub fn diff_and_update(&mut self, new_files: &HashMap<PathBuf, FileMeta>) {
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
        for (path, meta) in new_files {
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
            Self::update_redb_batch_commit(db, &to_remove, &to_add_or_update);
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
        if let Err(e) = std::io::stdout().flush() {
            eprintln!("[FileCache] Failed to flush stdout: {e}");
        }
    }
    fn update_redb_batch_commit(
        db: &redb::Database,
        to_remove: &[PathBuf],
        to_add_or_update: &[(PathBuf, FileMeta)],
    ) {
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("[FileCache] Failed to begin write txn: {e}");
                return;
            }
        };
        let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[FileCache] Failed to open file_cache table: {e}");
                return;
            }
        };
        for path in to_remove {
            if let Err(e) = table.remove(path.to_string_lossy().as_ref()) {
                eprintln!("[FileCache] Failed to remove file meta: {e}");
            }
        }
        for (path, meta) in to_add_or_update {
            if let Err(e) =
                table.insert(path.to_string_lossy().as_ref(), meta.serialize().as_slice())
            {
                eprintln!("[FileCache] Failed to insert/update file meta: {e}");
            }
        }
        drop(table);
        if let Err(e) = write_txn.commit() {
            eprintln!("[FileCache] Failed to commit batch diff update: {e}");
        }
    }
    /// Scan a directory and atomically update only changes (calls `diff_and_update`)
    pub fn scan_dir(&mut self, dir: &Path) {
        let new_files = Self::scan_dir_collect(dir);
        self.diff_and_update(&new_files);
    }
    /// Update or insert a file's metadata
    pub fn update_file(&mut self, path: &Path) {
        if let Some(meta) = FileMeta::from_path(path) {
            self.files.insert(path.to_path_buf(), meta.clone());
            if let Some(db) = &self.db {
                Self::update_redb_single_insert(db, path, &meta);
            }
        }
    }
    fn update_redb_single_insert(db: &redb::Database, path: &Path, meta: &FileMeta) {
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("[FileCache] Failed to begin write txn: {e}");
                return;
            }
        };
        let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[FileCache] Failed to open file_cache table: {e}");
                return;
            }
        };
        if let Err(e) = table.insert(path.to_string_lossy().as_ref(), meta.serialize().as_slice()) {
            eprintln!("[FileCache] Failed to insert/update file meta: {e}");
        }
        drop(table);
        if let Err(e) = write_txn.commit() {
            eprintln!("[FileCache] Failed to commit update: {e}");
        }
    }
    /// Remove a file from the cache
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(path);
        if let Some(db) = &self.db {
            Self::update_redb_single_remove(db, path);
        }
    }
    fn update_redb_single_remove(db: &redb::Database, path: &Path) {
        let write_txn = match db.begin_write() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("[FileCache] Failed to begin write txn: {e}");
                return;
            }
        };
        let mut table = match write_txn.open_table(FILE_CACHE_TABLE) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[FileCache] Failed to open file_cache table: {e}");
                return;
            }
        };
        if let Err(e) = table.remove(path.to_string_lossy().as_ref()) {
            eprintln!("[FileCache] Failed to remove file meta: {e}");
        }
        drop(table);
        if let Err(e) = write_txn.commit() {
            eprintln!("[FileCache] Failed to commit remove: {e}");
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
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    eprintln!("[FileCache] Failed to begin read txn: {e}");
                    return;
                }
            };
            let table = match read_txn.open_table(FILE_CACHE_TABLE) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[FileCache] Failed to open file_cache table: {e}");
                    return;
                }
            };
            self.files.clear();
            let range = match table.range::<&str>(..) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[FileCache] Failed to get table range: {e}");
                    return;
                }
            };
            for entry in range {
                match entry {
                    Ok((k, v)) => {
                        let path = PathBuf::from(k.value());
                        let meta = FileMeta::deserialize(v.value());
                        self.files.insert(path, meta);
                    }
                    Err(e) => {
                        eprintln!("[FileCache] Failed to read entry: {e}");
                    }
                }
            }
        }
    }
    #[allow(dead_code)]
    fn add_dir(&mut self, dir: &Path, depth: usize) {
        if depth > 10 {
            println!(
                "[FileCache] Max recursion depth reached at {}",
                dir.display()
            );
            return;
        }
        println!("[FileCache] add_dir: {}", dir.display());
        if let Err(e) = std::io::stdout().flush() {
            eprintln!("[FileCache] Failed to flush stdout: {e}");
        }
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let path = entry.path();
                            println!("[FileCache] entry: {}", path.display());
                            if let Err(e) = std::io::stdout().flush() {
                                eprintln!("[FileCache] Failed to flush stdout: {e}");
                            }
                            if path.is_dir() {
                                self.add_dir(&path, depth + 1);
                            } else if let Some(meta) = FileMeta::from_path(&path) {
                                self.files.insert(path, meta);
                            }
                        }
                        Err(e) => {
                            println!("[FileCache] Error reading entry in {}: {e}", dir.display());
                        }
                    }
                }
            }
            Err(e) => {
                println!("[FileCache] Error reading dir {}: {e}", dir.display());
            }
        }
    }
}
