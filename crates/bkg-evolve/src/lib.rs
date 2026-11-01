//! bkg-evolve — bkgclaw improving itself, in the open.
//!
//! The cycle, its git hands, its fitness judge and its memory live here.
//! Nothing in this crate grants itself a privilege: the engine uses the
//! same loop, the same gate classes and the same snapshots every other
//! run goes through — plus one rule that is evolution's own: **the model
//! never commits, the fitness gate commits.**

#![forbid(unsafe_code)]

pub mod daemon;
pub mod engine;
pub mod evolution;
pub mod fitness;
pub mod gitops;
pub mod loop_driver;
pub mod memory;

pub use daemon::{DaemonStats, Goal, MAX_GOAL_ATTEMPTS, Queue, now_secs};
pub use engine::{CycleResult, EvolveOptions, init_repo, journal_path, push_state, run_cycle};
pub use evolution::{EvolutionLoop, Proposal};
pub use fitness::{FitnessReport, Verdict, measure as measure_fitness};
pub use loop_driver::{
    Pass, changelog_path, changelog_tail, log_event, one_pass, queue_path, run_daemon, seed,
    stats_for,
};
pub use memory::{Attempt, AttemptArchive};
