//! Cleave orchestrator — parallel child dispatch with worktree isolation.
//!
//! Replaces the TypeScript dispatcher (extensions/cleave/dispatcher.ts).
//! Spawns omegon-agent children in git worktrees, manages dependency waves,
//! tracks state, and merges results.

pub mod context;
pub mod coverage;
pub mod guardrails;
mod plan;
pub mod progress;
pub mod state;
pub mod test_architect;
mod waves;
mod worktree;
pub mod orchestrator;

pub use orchestrator::run_cleave;
pub use plan::CleavePlan;
