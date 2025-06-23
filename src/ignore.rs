// Provides configuration for directories/files to ignore during filesystem scanning/watching

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub type IgnoreConfigResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Holds the set of ignore patterns for the scanner.
pub struct IgnoreConfig {
    gitignore: Gitignore,
    patterns: Vec<String>,
}

impl IgnoreConfig {
    /// Create a new ignoreConfig from a list of glob pattern strings.
    pub fn new(patterns: &[&str]) -> IgnoreConfigResult<Self> {
        let mut builder = GitignoreBuilder::new("");
        for pat in patterns {
            builder.add_line(None, pat)?;
        }
        let gitignore = builder
            .build()
            .map_err(|e| format!("Gitignore build error: {e}"))?;
        Ok(IgnoreConfig {
            gitignore,
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Load ignore patterns from a config file (like .gitignore)
    /// Returns both the ignoreConfig and the loaded patterns for logging.
    pub fn from_file_with_patterns<P: AsRef<Path>>(
        path: P,
    ) -> IgnoreConfigResult<(Self, Vec<String>)> {
        let file = File::open(path.as_ref())?;
        let reader = BufReader::new(file);
        let mut builder = GitignoreBuilder::new("");
        let mut patterns = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            builder.add_line(None, trimmed)?;
            patterns.push(trimmed.to_string());
        }
        let gitignore = builder
            .build()
            .map_err(|e| format!("Gitignore build error: {e}"))?;
        Ok((
            IgnoreConfig {
                gitignore,
                patterns: patterns.clone(),
            },
            patterns,
        ))
    }

    /// Returns true if the given path should be ignoreped.
    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        self.gitignore.matched(path, path.is_dir()).is_ignore()
    }

    /// Returns the patterns for logging/debugging.
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Creates an empty `ignoreConfig` with no patterns.
    pub fn empty() -> Self {
        IgnoreConfig {
            gitignore: ignore::gitignore::Gitignore::empty(),
            patterns: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_ignore_config() {
        let config = IgnoreConfig::new(&["*.tmp", "target/", "**/node_modules/"]).unwrap();
        assert!(config.is_ignored("foo.tmp"));
        assert!(config.is_ignored("target/build.log"));
        assert!(config.is_ignored("src/node_modules/bar.js"));
        assert!(!config.is_ignored("src/main.rs"));
    }
}
