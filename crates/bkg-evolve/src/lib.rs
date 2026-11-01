//! bkg-evolve — bkgclaw improving itself, in the open.
//!
//! The cycle, its git hands, its fitness judge and its memory live here.
//! Nothing in this crate grants itself a privilege: the engine uses the
//! same loop, the same gate classes and the same snapshots every other
//! run goes through — plus one rule that is evolution's own: **the model
//! never commits, the fitness gate commits.**

#![forbid(unsafe_code)]

pub mod engine;
pub mod evolution;
pub mod fitness;
pub mod gitops;
pub mod memory;

pub use engine::{CycleResult, EvolveOptions, init_repo, journal_path, push_state, run_cycle};
pub use evolution::{EvolutionLoop, Proposal};
pub use fitness::{FitnessReport, Verdict, measure as measure_fitness};
pub use memory::{Attempt, AttemptArchive};
