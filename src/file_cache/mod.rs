//! `file_cache` module root

pub mod cache;
pub mod db;
pub mod meta;

pub use cache::FileCache;
pub use meta::FileMeta;
// FileCachePath is not re-exported unless needed externally
