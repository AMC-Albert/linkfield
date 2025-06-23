use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct FileEvent {
    pub path: PathBuf,
    pub kind: FileEventKind,
    pub time: Instant,
    pub size: Option<u64>,
    pub metadata: Option<fs::Metadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEventKind {
    Remove,
    Create,
}

#[derive(Debug, Clone)]
pub struct MoveCandidate {
    pub from: FileEvent,
    pub to: FileEvent,
    pub score: f64,
}

/// Heuristic for pairing Remove/Create events as moves.
pub struct MoveHeuristics {
    pub remove_events: VecDeque<FileEvent>,
    pub max_age: Duration,
}

impl MoveHeuristics {
    pub fn new(max_age: Duration) -> Self {
        Self {
            remove_events: VecDeque::new(),
            max_age,
        }
    }

    /// Add a Remove event to the cache
    pub fn add_remove(&mut self, event: FileEvent) {
        self.remove_events.push_back(event);
        self.prune_old();
    }

    /// Try to pair a Create event with a cached Remove event
    pub fn pair_create(&mut self, create: &FileEvent) -> Option<MoveCandidate> {
        self.prune_old();
        let mut best: Option<MoveCandidate> = None;
        for remove in self.remove_events.iter() {
            let score = score_pair(remove, create);
            eprintln!(
                "[Heuristics] Score for Remove({:?}) <-> Create({:?}): {:.2}",
                remove.path, create.path, score
            );
            if score > 0.5 {
                // Good enough match
                let candidate = MoveCandidate {
                    from: remove.clone(),
                    to: create.clone(),
                    score,
                };
                if best.as_ref().map_or(true, |b| score > b.score) {
                    best = Some(candidate);
                }
            }
        }
        if let Some(ref best_candidate) = best {
            // Remove the paired Remove event
            if let Some(pos) = self
                .remove_events
                .iter()
                .position(|e| e.path == best_candidate.from.path)
            {
                self.remove_events.remove(pos);
            }
        }
        best
    }

    fn prune_old(&mut self) {
        let now = Instant::now();
        self.remove_events
            .retain(|e| now.duration_since(e.time) < self.max_age);
    }
}

/// Score a Remove/Create pair for likelihood of being a move
pub fn score_pair(remove: &FileEvent, create: &FileEvent) -> f64 {
    let mut score = 0.0;
    // File size match is strong evidence
    if let (Some(rs), Some(cs)) = (remove.size, create.size) {
        if rs == cs && rs > 0 {
            score += 0.7;
        } else if (rs as i64 - cs as i64).abs() < 16 {
            score += 0.4;
        }
    }
    // File extension match
    if remove.path.extension() == create.path.extension() {
        score += 0.2;
    }
    // File name similarity (Levenshtein or prefix match)
    if let (Some(rn), Some(cn)) = (remove.path.file_name(), create.path.file_name()) {
        let rn = rn.to_string_lossy();
        let cn = cn.to_string_lossy();
        if rn == cn {
            score += 0.2;
        } else if rn.as_ref().starts_with(cn.as_ref()) || cn.as_ref().starts_with(rn.as_ref()) {
            score += 0.1;
        }
    }
    // Timestamps (if available)
    if let (Some(rm), Some(cm)) = (&remove.metadata, &create.metadata) {
        if let (Ok(rmt), Ok(cmt)) = (rm.modified(), cm.modified()) {
            if (rmt.duration_since(cmt).unwrap_or_default().as_secs() < 2)
                || (cmt.duration_since(rmt).unwrap_or_default().as_secs() < 2)
            {
                score += 0.1;
            }
        }
    }
    let score = (score as f64).min(1.0f64);
    score
}

/// Helper to create a FileEvent from a path and kind
pub fn make_file_event(path: PathBuf, kind: FileEventKind) -> FileEvent {
    let metadata = fs::metadata(&path).ok();
    let size = metadata.as_ref().map(|m| m.len());
    FileEvent {
        path,
        kind,
        time: Instant::now(),
        size,
        metadata,
    }
}
