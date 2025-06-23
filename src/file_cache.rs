use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Debug, Clone)]
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
}

/// A cache of file metadata for all watched files.
pub struct FileCache {
    files: HashMap<PathBuf, FileMeta>,
    last_scan: Instant,
}

impl FileCache {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            last_scan: Instant::now(),
        }
    }

    /// Scan a directory recursively and cache all files
    pub fn scan_dir(&mut self, dir: &Path) {
        self.files.clear();
        self.last_scan = Instant::now();
        self.add_dir(dir);
    }

    fn add_dir(&mut self, dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    self.add_dir(&path);
                } else if let Some(meta) = FileMeta::from_path(&path) {
                    self.files.insert(path, meta);
                }
            }
        }
    }

    /// Update or insert a file's metadata
    pub fn update_file(&mut self, path: &Path) {
        if let Some(meta) = FileMeta::from_path(path) {
            self.files.insert(path.to_path_buf(), meta);
        }
    }

    /// Remove a file from the cache
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(path);
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
}
