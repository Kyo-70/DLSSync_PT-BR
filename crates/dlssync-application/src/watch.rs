use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

pub const WATCH_TRAILING_EDGE: Duration = Duration::from_millis(500);
pub const WATCH_BURST_CAP: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyPaths {
    paths: BTreeSet<PathBuf>,
    first_event_ms: Option<u64>,
    last_event_ms: Option<u64>,
}

impl DirtyPaths {
    pub fn record(&mut self, path: PathBuf, monotonic_ms: u64) {
        self.paths.insert(path);
        self.first_event_ms.get_or_insert(monotonic_ms);
        self.last_event_ms = Some(monotonic_ms);
    }

    pub fn due(&self, monotonic_ms: u64) -> bool {
        self.last_event_ms.is_some_and(|last| {
            monotonic_ms.saturating_sub(last) >= WATCH_TRAILING_EDGE.as_millis() as u64
                || self.first_event_ms.is_some_and(|first| {
                    monotonic_ms.saturating_sub(first) >= WATCH_BURST_CAP.as_millis() as u64
                })
        })
    }

    pub fn take_due(&mut self, monotonic_ms: u64) -> Option<Vec<PathBuf>> {
        if !self.due(monotonic_ms) {
            return None;
        }
        let paths = std::mem::take(&mut self.paths).into_iter().collect();
        self.first_event_ms = None;
        self.last_event_ms = None;
        Some(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase4_watch_coalesces_at_500ms_after_final_event() {
        let mut dirty = DirtyPaths::default();
        dirty.record(PathBuf::from("a.dll"), 0);
        dirty.record(PathBuf::from("b.dll"), 100);
        dirty.record(PathBuf::from("c.dll"), 400);
        assert!(dirty.take_due(899).is_none());
        let paths = dirty.take_due(900).unwrap();
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn phase4_watch_caps_continuous_burst_at_1500ms() {
        let mut dirty = DirtyPaths::default();
        dirty.record(PathBuf::from("a.dll"), 0);
        dirty.record(PathBuf::from("a.dll"), 1400);
        assert!(dirty.take_due(1499).is_none());
        assert_eq!(dirty.take_due(1500).unwrap(), vec![PathBuf::from("a.dll")]);
    }
}
