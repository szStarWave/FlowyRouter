use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Tracks in-flight edge upstream inference calls (local GPU / Ollama typically serial).
#[derive(Debug, Default)]
pub struct EdgeInferenceTracker {
    in_flight: AtomicUsize,
}

impl EdgeInferenceTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            in_flight: AtomicUsize::new(0),
        })
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight() > 0
    }

    pub fn begin(self: &Arc<Self>) -> EdgeInferenceGuard {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        EdgeInferenceGuard {
            tracker: Arc::clone(self),
        }
    }
}

pub struct EdgeInferenceGuard {
    tracker: Arc<EdgeInferenceTracker>,
}

impl Drop for EdgeInferenceGuard {
    fn drop(&mut self) {
        self.tracker.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_while_guard_held() {
        let t = EdgeInferenceTracker::new();
        assert!(!t.is_busy());
        let g = t.begin();
        assert!(t.is_busy());
        assert_eq!(t.in_flight(), 1);
        drop(g);
        assert!(!t.is_busy());
    }
}
