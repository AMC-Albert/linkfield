//! `FileCache`: in-memory and persistent file metadata cache

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

use crate::file_cache::db;
use crate::file_cache::meta::{FileCachePath, FileMeta};
use linkfield::ignore::IgnoreConfig;

/// `FileCache`: stores file metadata in memory and optionally in redb
pub struct FileCache {
    files: HashMap<FileCachePath, FileMeta>,
    last_scan: Instant,
    db: Option<redb::Database>,
}

impl FileCache {
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
    pub fn scan_dir_collect(dir: &Path) -> HashMap<FileCachePath, FileMeta> {
        let scan_span = tracing::info_span!("scan_dir_collect", dir = %dir.display());
        let _scan_enter = scan_span.enter();
        tracing::info!("scan_dir_collect: {}", dir.display());
        let counter = Arc::new(AtomicUsize::new(0));
        let pb = ProgressBar::new_spinner();
        if let Ok(style) = ProgressStyle::with_template("{spinner:.green} Scanning files: {pos}") {
            pb.set_style(style);
        } else {
            tracing::warn!("Failed to set progress bar style");
        }
        let files = Self::collect_files_parallel_progress(dir, 0, &counter, &pb);
        pb.finish_with_message("Scan complete");
        tracing::info!(count = counter.load(Ordering::Relaxed), "Scanned files");
        files.into_iter().collect()
    }
    fn collect_files_parallel_progress(
        dir: &Path,
        _depth: usize,
        counter: &Arc<AtomicUsize>,
        pb: &ProgressBar,
    ) -> Vec<(FileCachePath, FileMeta)> {
        use rayon::iter::ParallelBridge;
        let span = tracing::info_span!("collect_files_parallel_progress", dir = %dir.display());
        let _enter = span.enter();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e.par_bridge().filter_map(Result::ok).collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(error = %e, dir = %dir.display(), "Error reading dir");
                return Vec::new();
            }
        };
        let (dirs, files): (Vec<_>, Vec<_>) = entries
            .into_par_iter()
            .partition(|entry| entry.path().is_dir());
        let mut results: Vec<(FileCachePath, FileMeta)> = files
            .into_par_iter()
            .filter_map(|entry| {
                let path = entry.path();
                let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 100 == 0 {
                    pb.set_position(count as u64);
                }
                FileMeta::from_path(&path).map(|meta| (FileCachePath::from(path.as_path()), meta))
            })
            .collect();
        let mut sub_results: Vec<(FileCachePath, FileMeta)> = dirs
            .into_par_iter()
            .flat_map_iter(|entry| {
                Self::collect_files_parallel_progress(&entry.path(), 0, counter, pb)
            })
            .collect();
        results.append(&mut sub_results);
        results
    }
    pub fn diff_and_update(&mut self, new_files: &HashMap<FileCachePath, FileMeta>) {
        let mut added = 0;
        let mut updated = 0;
        let mut unchanged = 0;
        let mut to_remove = Vec::new();
        let mut to_add_or_update = Vec::new();
        for old_path in self.files.keys() {
            if !new_files.contains_key(old_path) {
                to_remove.push(old_path.clone());
            }
        }
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
        if let Some(db) = &self.db {
            db::update_redb_batch_commit(db, &to_remove, &to_add_or_update);
        }
        for path in &to_remove {
            self.files.remove(path);
        }
        for (path, meta) in &to_add_or_update {
            self.files.insert(path.clone(), meta.clone());
        }
        tracing::info!(added, updated, unchanged, "[FileCache][diff]");
    }
    pub fn scan_dir(&mut self, dir: &Path) {
        let new_files = Self::scan_dir_collect(dir);
        self.diff_and_update(&new_files);
    }
    pub fn update_file(&mut self, path: &Path) {
        if let Some(meta) = FileMeta::from_path(path) {
            let key = FileCachePath::from(path);
            self.files.insert(key.clone(), meta.clone());
            if let Some(db) = &self.db {
                db::update_redb_single_insert(db, &key, &meta);
            }
        }
    }
    pub fn remove_file(&mut self, path: &Path) {
        let key = FileCachePath::from(path);
        self.files.remove(&key);
        if let Some(db) = &self.db {
            db::update_redb_single_remove(db, &key);
        }
    }
    pub fn get(&self, path: &Path) -> Option<&FileMeta> {
        let key = FileCachePath::from(path);
        self.files.get(&key)
    }
    pub fn all_files(&self) -> impl Iterator<Item = &FileMeta> {
        self.files.values()
    }
    pub fn load_from_redb(&mut self) {
        if let Some(db) = &self.db {
            let read_txn = match db.begin_read() {
                Ok(txn) => txn,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to begin read txn");
                    return;
                }
            };
            let table = match read_txn.open_table(db::FILE_CACHE_TABLE) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to open file_cache table");
                    return;
                }
            };
            self.files.clear();
            let range = match table.range::<&str>(..) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to get table range");
                    return;
                }
            };
            for entry in range {
                match entry {
                    Ok((k, v)) => {
                        let path = FileCachePath(std::path::PathBuf::from(k.value()));
                        let meta = FileMeta::deserialize(v.value());
                        self.files.insert(path, meta);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to read entry");
                    }
                }
            }
        }
    }
    pub fn scan_dir_collect_with_ignore(
        dir: &Path,
        ignore: &IgnoreConfig,
    ) -> HashMap<FileCachePath, FileMeta> {
        let scan_span = tracing::info_span!("scan_dir_collect", dir = %dir.display());
        let _scan_enter = scan_span.enter();
        tracing::info!("scan_dir_collect: {}", dir.display());
        let counter = Arc::new(AtomicUsize::new(0));
        let pb = ProgressBar::new_spinner();
        if let Ok(style) = ProgressStyle::with_template("{spinner:.green} Scanning files: {pos}") {
            pb.set_style(style);
        } else {
            tracing::warn!("Failed to set progress bar style");
        }
        let files =
            Self::collect_files_parallel_progress_with_ignore(dir, 0, &counter, &pb, ignore);
        pb.finish_with_message("Scan complete");
        tracing::info!(count = counter.load(Ordering::Relaxed), "Scanned files");
        files.into_iter().collect()
    }
    fn collect_files_parallel_progress_with_ignore(
        dir: &Path,
        _depth: usize,
        counter: &Arc<AtomicUsize>,
        pb: &ProgressBar,
        ignore: &IgnoreConfig,
    ) -> Vec<(FileCachePath, FileMeta)> {
        use rayon::iter::ParallelBridge;
        let span = tracing::info_span!("collect_files_parallel_progress", dir = %dir.display());
        let _enter = span.enter();
        if ignore.is_ignored(dir) {
            tracing::warn!(ignore_match = %dir.display(), "ignoring directory due to ignore config");
            return Vec::new();
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e.par_bridge().filter_map(Result::ok).collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(error = %e, dir = %dir.display(), "Error reading dir");
                return Vec::new();
            }
        };
        let (dirs, files): (Vec<_>, Vec<_>) = entries
            .into_par_iter()
            .partition(|entry| entry.path().is_dir());
        let mut results: Vec<(FileCachePath, FileMeta)> = files
            .into_par_iter()
            .filter_map(|entry| {
                let path = entry.path();
                if ignore.is_ignored(&path) {
                    tracing::info!(ignore_match = %path.display(), "ignoring file due to ignore config");
                    return None;
                }
                let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if count % 100 == 0 {
                    pb.set_position(count as u64);
                }
                // Fix: convert &PathBuf to &Path for FileCachePath::from
                FileMeta::from_path(&path).map(|meta| (FileCachePath::from(path.as_path()), meta))
            })
            .collect();
        let mut sub_results: Vec<(FileCachePath, FileMeta)> = dirs
            .into_par_iter()
            .flat_map_iter(|entry| {
                Self::collect_files_parallel_progress_with_ignore(
                    &entry.path(),
                    0,
                    counter,
                    pb,
                    ignore,
                )
            })
            .collect();
        results.append(&mut sub_results);
        results
    }
}
