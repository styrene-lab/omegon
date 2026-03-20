//! auto_compact — Proactive context compaction.
//!
//! Monitors estimated context usage after each turn and requests compaction
//! before the context window fills up. Fires at a configurable percentage
//! threshold (default 70%) with a cooldown between compactions.
//!
//! Ported from: extensions/auto-compact.ts (39 LoC → 56 LoC)

use async_trait::async_trait;
use omegon_traits::{BusEvent, BusRequest, Feature};
use std::time::Instant;

const DEFAULT_THRESHOLD_PERCENT: f32 = 70.0;
const DEFAULT_COOLDOWN_SECS: u64 = 60;

/// Proactive context compaction feature.
pub struct AutoCompact {
    threshold: f32,
    cooldown: std::time::Duration,
    last_compact: Option<Instant>,
    compacting: bool,
    /// Estimated context usage from the agent loop.
    estimated_percent: f32,
}

impl Default for AutoCompact {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoCompact {
    pub fn new() -> Self {
        let threshold = std::env::var("AUTO_COMPACT_PERCENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_THRESHOLD_PERCENT);
        let cooldown_secs = std::env::var("AUTO_COMPACT_COOLDOWN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_COOLDOWN_SECS);

        Self {
            threshold,
            cooldown: std::time::Duration::from_secs(cooldown_secs),
            last_compact: None,
            compacting: false,
            estimated_percent: 0.0,
        }
    }
}

#[async_trait]
impl Feature for AutoCompact {
    fn name(&self) -> &str {
        "auto-compact"
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::TurnEnd { turn } => {
                if self.compacting {
                    return vec![];
                }

                // Rough estimate: ~2k tokens per turn
                // The real estimate comes from the context manager, but we
                // don't have direct access here. Use turn count as proxy.
                // TODO: BusEvent::TurnEnd should carry context usage stats.
                self.estimated_percent = (*turn as f32) * 2.0; // rough %

                if self.estimated_percent < self.threshold {
                    return vec![];
                }

                if self.last_compact.is_some_and(|last| last.elapsed() < self.cooldown) {
                    return vec![];
                }

                self.compacting = true;
                self.last_compact = Some(Instant::now());
                tracing::info!(
                    turn = turn,
                    threshold = self.threshold,
                    "auto-compact: requesting compaction"
                );

                vec![BusRequest::RequestCompaction]
            }
            BusEvent::Compacted => {
                self.compacting = false;
                vec![]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_compact_below_threshold() {
        let mut ac = AutoCompact::new();
        let requests = ac.on_event(&BusEvent::TurnEnd { turn: 1 });
        assert!(requests.is_empty(), "turn 1 should not trigger compaction");
    }

    #[test]
    fn compacts_above_threshold() {
        let mut ac = AutoCompact::new();
        ac.threshold = 50.0; // lower threshold for testing
        // Turn 30 → estimated 60% > 50%
        let requests = ac.on_event(&BusEvent::TurnEnd { turn: 30 });
        assert_eq!(requests.len(), 1);
        assert!(matches!(requests[0], BusRequest::RequestCompaction));
    }

    #[test]
    fn cooldown_prevents_repeated_compaction() {
        let mut ac = AutoCompact::new();
        ac.threshold = 10.0;

        let r1 = ac.on_event(&BusEvent::TurnEnd { turn: 10 });
        assert_eq!(r1.len(), 1, "first compaction should fire");

        // Mark compaction complete
        ac.on_event(&BusEvent::Compacted);

        // Immediately try again — cooldown should prevent
        let r2 = ac.on_event(&BusEvent::TurnEnd { turn: 11 });
        assert!(r2.is_empty(), "cooldown should prevent immediate re-compact");
    }

    #[test]
    fn compacted_event_clears_flag() {
        let mut ac = AutoCompact::new();
        ac.compacting = true;

        ac.on_event(&BusEvent::Compacted);
        assert!(!ac.compacting);
    }
}
