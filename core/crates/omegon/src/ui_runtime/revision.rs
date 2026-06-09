//! UI runtime revision types.
//!
//! Revisions are deterministic causal ordering for surface/action streams. They
//! are not wall-clock time, simulation time, or replay playback pacing.

/// Monotonic UI runtime revision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UiRevision(u64);

impl UiRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<UiRevision> for u64 {
    fn from(value: UiRevision) -> Self {
        value.get()
    }
}

/// Monotonic revision allocator for UI replay fixtures and runtime streams.
#[derive(Debug, Clone, Default)]
pub struct UiRevisionCounter {
    current: UiRevision,
}

impl UiRevisionCounter {
    pub const fn new() -> Self {
        Self {
            current: UiRevision::new(0),
        }
    }

    pub const fn current(&self) -> UiRevision {
        self.current
    }

    pub fn next_revision(&mut self) -> UiRevision {
        let next = self
            .current
            .get()
            .checked_add(1)
            .expect("UI revision exhausted");
        self.current = UiRevision::new(next);
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_revision_counter_allocates_global_monotonic_revisions() {
        let mut counter = UiRevisionCounter::new();

        assert_eq!(counter.current().get(), 0);
        assert_eq!(counter.next_revision().get(), 1);
        assert_eq!(counter.next_revision().get(), 2);
        assert_eq!(counter.current().get(), 2);
    }

    #[test]
    fn ui_revision_converts_to_u64_for_envelopes() {
        let revision = UiRevision::new(42);
        let value: u64 = revision.into();

        assert_eq!(value, 42);
    }
}
