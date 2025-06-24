//! File metadata for the file cache module

use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Strongly typed file path wrapper for cache keys
#[derive(Debug, Clone, PartialEq, Eq, Hash, Encode, Decode)]
pub struct FileCachePath(pub PathBuf);

impl From<&Path> for FileCachePath {
	fn from(path: &Path) -> Self {
		Self(path.to_path_buf())
	}
}

impl AsRef<Path> for FileCachePath {
	fn as_ref(&self) -> &Path {
		&self.0
	}
}

/// Metadata for a single file in the cache
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct FileMeta {
	pub path: FileCachePath,
	pub size: u64,
	pub modified: Option<SystemTime>,
	pub created: Option<SystemTime>,
	pub extension: Option<String>,
}

impl FileMeta {
	pub fn from_path(path: &Path) -> Option<Self> {
		let metadata = fs::metadata(path).ok()?;
		Some(Self {
			path: FileCachePath::from(path),
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
			tracing::error!(error = %e, "Serialization failed");
			Vec::new()
		})
	}
	pub fn deserialize(bytes: &[u8]) -> Self {
		let (meta, _) = decode_from_slice(bytes, bincode::config::standard()).unwrap_or_else(|e| {
			tracing::error!(error = %e, "Deserialization failed");
			(
				Self {
					path: FileCachePath(PathBuf::new()),
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
