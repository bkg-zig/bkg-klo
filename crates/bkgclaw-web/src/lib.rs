//! bkgclaw-web — the cockpit in the browser.
//!
//! One client, two channels, the same protocol as the TUI:
//!
//! - REST (`gloo-net`) for the slow facts: sessions, models, tasks, memory,
//!   skills and a session's transcript.
//! - WebSocket (`web-sys`) for the live run: streaming deltas, tool
//!   outcomes, approval requests, cancellation.
//!
//! The UI is deliberately server-thin: no state lives here that the
//! gateway does not already own. Reload the page, and everything is back.

use serde::Deserialize;

// The wire contract, imported from the crate that owns it. This used to be a
// hand-maintained copy of the gateway's enums — see the note in Cargo.toml for
// why a copy of a protocol is worse than no protocol at all.
pub use bkgclaw_protocol::{
    ApprovalDecision, ClientMessage as WireClient, Event as WireEvent,
};

mod net;
pub mod views;

/// Everything the cockpit knows, in one signal bundle. Dioxus signals make
/// the clone-heavy parts of UI state cheap; the gateway stays the truth.
#[derive(Debug, Clone, Default)]
pub struct Cockpit {
    pub base: String,
    pub token: Option<String>,
    pub session: Option<String>,
    pub models: Vec<String>,
    pub sessions: Vec<SessionRow>,
    pub transcript: Vec<TranscriptItem>,
    pub live: String,
    pub live_reasoning: String,
    pub busy: bool,
    /// The pending approval, in the protocol's own shape. The `session` field
    /// rides along because the event carries it; the modal does not show it.
    pub pending_approval: Option<bkgclaw_protocol::Event>,
    pub spent: f64,
    pub notice: String,
    pub panel: Panel,
    pub tasks: Vec<TaskRow>,
    pub memory: Vec<MemoryRow>,
    pub skills: Vec<SkillRow>,
    /// The evolve daemon's state. Populated on demand, not polled: the
    /// operator opens this panel to look, not to stare at a counter.
    pub evolve: crate::EvolveReport,
    pub input: String,
    pub policy: String,
    /// A session created from this page is adopted when its confirmation
    /// event arrives.
    pub pending_new: bool,
    /// The model chosen for the next session.
    pub chosen_model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Panel {
    #[default]
    Chat,
    Tasks,
    Memory,
    Skills,
    /// What the self-improvement daemon has been doing: counters, the queue
    /// it is working through, and the attempt archive with its lessons.
    Evolve,
}

/// One rendered transcript row.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptItem {
    User(String),
    Assistant(String),
    Reasoning(String),
    Tool { name: String, outcome: String, output: String },
    Error(String),
    /// A daemon notice addressed to every client — evolve progress, snapshots.
    /// Distinguished from `Error` because it is information, not failure.
    Status(String),
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SessionRow {
    pub id: String,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub message_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TaskRow {
    pub slug: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryRow {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillRow {
    pub name: String,
    pub description: String,
}

/// What the evolve daemon has done since it started.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct EvolveDaemon {
    #[serde(default)]
    pub cycles: u64,
    #[serde(default)]
    pub improved: u64,
    #[serde(default)]
    pub unchanged: u64,
    #[serde(default)]
    pub failed: u64,
    /// The goal being worked right now, if any. `None` between cycles is
    /// normal; `Some` that never moves is a stuck daemon.
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub last_cycle_ts: u64,
    #[serde(default)]
    pub started_ts: u64,
    /// True when no push remote is configured. Shown prominently: an agent
    /// improving a tree nobody can see is a machine burning a budget quietly.
    #[serde(default)]
    pub origin_missing: bool,
}

/// One goal on the daemon's queue.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct EvolveGoal {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub push: bool,
    /// Retried too often to keep trying; the goal is kept, not deleted.
    #[serde(default)]
    pub parked: bool,
}

/// One finished evolution attempt.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct EvolveAttempt {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub branch: String,
    /// `improved` | `neutral` | `suspect` | `reverted`
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub tests_passed: u64,
    #[serde(default)]
    pub tests_failed: u64,
    #[serde(default)]
    pub clippy_warnings: u64,
    #[serde(default)]
    pub lesson: String,
}

/// Everything `/api/evolve` returns, in one shape.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct EvolveReport {
    #[serde(default)]
    pub daemon: EvolveDaemon,
    #[serde(default)]
    pub queue: Vec<EvolveGoal>,
    #[serde(default)]
    pub attempts: Vec<EvolveAttempt>,
    #[serde(default)]
    pub journal_head: String,
    #[serde(default)]
    pub queue_path: String,
}
