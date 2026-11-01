//! The continuous evolve loop: a queue of goals, worked through forever.
//!
//! # Why this exists
//!
//! Evolution so far was one-shot: a human ran `bkgclaw evolve run "…"`, a
//! cycle happened, and the machine went back to waiting. That makes the
//! self-improvement loop a thing you *start*, and a thing that stops. The point
//! of a self-improving system is that it does not need someone standing there.
//!
//! So the daemon takes goals from a queue file and works through them without
//! stopping: read a goal, run a cycle, record the result, read the next. It
//! survives failures — a cycle that fails is *data*, not a crash — and it stops
//! when the queue is empty and no new goal has arrived.
//!
//! # The queue is a file, not a channel
//!
//! ```text
//!   $BKGCLAW_HOME/evolve/queue.jsonl   one goal per line
//! ```
//!
//! Append-only, one JSON object per line, so an operator can add work with
//! `echo` while the daemon runs, and a crash mid-write costs at most the line
//! being written. A channel would hold goals in memory, and a restart would
//! silently drop them — the exact failure a "durable queue" is supposed to
//! prevent.
//!
//! # The rules it obeys
//!
//! 1. **One cycle at a time.** Two concurrent cycles on one tree would fight
//!    over the same branch and the same snapshot store.
//! 2. **Every cycle ends in a fitness verdict**, which is what makes the loop
//!    safe to run unattended: a regression gets reverted, not committed.
//! 3. **A goal that fails three times is parked**, not retried forever. An
//!    unreachable goal would otherwise burn a model budget at 03:00 with
//!    nothing to show for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Seconds since the epoch. Shared with the loop driver so the two agree on
/// what a timestamp means.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// The bkgclaw home directory — where the queue, the archive and the running
/// log live.
pub fn home_root() -> PathBuf {
    bkgclaw_store::home_root()
}

/// One unit of queued work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    /// What to improve, in the agent's own words.
    pub goal: String,
    /// Working directory; defaults to the daemon's own tree.
    #[serde(default)]
    pub dir: Option<String>,
    /// Push the winning branch. Off by default: pushing is a decision.
    #[serde(default)]
    pub push: bool,
    /// How many times this goal has been tried and failed.
    #[serde(default)]
    pub attempts: u32,
}

/// After this many failures a goal is parked rather than retried.
pub const MAX_GOAL_ATTEMPTS: u32 = 3;

/// How long the daemon waits before looking at the queue again.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// What the daemon reports about itself.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DaemonStats {
    /// Cycles finished, whatever the verdict.
    pub cycles: u64,
    /// Cycles that ended `Committed`.
    pub improved: u64,
    /// Cycles that ended `Neutral` or `Reverted`.
    pub unchanged: u64,
    /// Cycles that could not run at all (model unavailable, tree dirty).
    pub failed: u64,
    /// The goal currently being worked, if any.
    pub current: Option<String>,
    /// When the last cycle finished, seconds since the epoch.
    pub last_cycle_ts: u64,
    /// When the daemon started, seconds since the epoch.
    pub started_ts: u64,
    /// Set when the daemon runs without a push remote. Shown in the UI,
    /// because "it improved 40 times and you never see it" is the most
    /// expensive kind of silence.
    pub origin_missing: bool,
}

/// The queue and the counters.
///
/// The counters live *inside* the queue behind interior mutability, not beside
/// it. That is the difference between a caller that has to thread `&mut
/// Queue` through every pass and one that cannot get the bookkeeping wrong:
/// `Queue::new` is the only way to make one, and every counter update goes
/// through a method that knows which counter it is.
#[derive(Debug)]
pub struct Queue {
    path: PathBuf,
    stats: std::sync::Mutex<DaemonStats>,
}

impl Queue {
    pub fn new(home: &Path) -> Self {
        let dir = home.join("evolve");
        let _ = std::fs::create_dir_all(&dir);
        Queue {
            path: dir.join("queue.jsonl"),
            stats: std::sync::Mutex::new(DaemonStats::default()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The counters, or a zeroed copy if the lock was poisoned by a panic in
    /// another thread. A daemon whose stats lock is poisoned should keep
    /// evolving, not deadlock — losing the numbers is cheaper than losing the
    /// loop.
    pub fn stats(&self) -> DaemonStats {
        self.stats
            .lock()
            .map(|stats| stats.clone())
            .unwrap_or_default()
    }

    /// Apply one change to the counters.
    pub fn bump<F: FnOnce(&mut DaemonStats)>(&self, change: F) {
        if let Ok(mut stats) = self.stats.lock() {
            change(&mut stats);
        }
    }

    /// Set the goal currently being worked.
    pub fn set_current(&self, goal: Option<String>) {
        self.bump(|stats| stats.current = goal);
    }

    /// Record that one cycle finished, whatever its verdict.
    pub fn record_cycle(&self) {
        self.bump(|stats| {
            stats.cycles += 1;
            stats.last_cycle_ts = now_secs();
        });
    }

    /// Every goal currently queued, oldest first.
    ///
    /// A malformed line is skipped rather than fatal: the queue is written by
    /// humans and other agents, and one bad line must not stop the daemon.
    pub fn goals(&self) -> Vec<Goal> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The next goal worth working: the first that has not been tried too
    /// often. Goals that exhausted their attempts are skipped, not deleted —
    /// deleting would lose the operator's work silently.
    pub fn next_goal(&self) -> Option<Goal> {
        self.goals()
            .into_iter()
            .find(|goal| goal.attempts < MAX_GOAL_ATTEMPTS)
    }

    /// Replace the file with `goals`, atomically.
    ///
    /// Write-to-temp-then-rename: a crash mid-write leaves the old queue
    /// intact instead of a truncated one. The daemon reads this file on every
    /// poll, so a half-written queue would drop goals.
    pub fn write(&self, goals: &[Goal]) -> Result<(), String> {
        let temporary = self.path.with_extension("jsonl.tmp");
        let body = goals
            .iter()
            .map(|goal| serde_json::to_string(goal).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&temporary, body).map_err(|e| format!("queue schreiben: {e}"))?;
        std::fs::rename(&temporary, &self.path).map_err(|e| format!("queue umbenennen: {e}"))?;
        Ok(())
    }

    /// Count one more failed attempt for `goal`.
    pub fn record_failure(&self, goal: &Goal) -> Result<(), String> {
        let mut goals = self.goals();
        if let Some(entry) = goals
            .iter_mut()
            .find(|entry| entry.goal == goal.goal && entry.dir == goal.dir)
        {
            entry.attempts = entry.attempts.saturating_add(1);
        }
        self.write(&goals)
    }

    /// Take `goal` out of the queue — it is done, or parked.
    pub fn remove(&self, goal: &Goal) -> Result<(), String> {
        let goals: Vec<Goal> = self
            .goals()
            .into_iter()
            .filter(|entry| !(entry.goal == goal.goal && entry.dir == goal.dir))
            .collect();
        self.write(&goals)
    }

    /// Aggregate counters from the archive, so the UI can show history without
    /// a second file to keep in sync.
    pub fn archive_summary(home: &Path) -> HashMap<&'static str, u64> {
        let archive = crate::AttemptArchive::new(home);
        let attempts = archive.all();
        let committed = attempts
            .iter()
            .filter(|attempt| attempt.verdict == crate::Verdict::Improved)
            .count() as u64;
        let total = attempts.len() as u64;
        let best_tests = attempts
            .iter()
            .map(|attempt| attempt.fitness.tests_passed)
            .max()
            .unwrap_or(0);
        let mut summary = HashMap::new();
        summary.insert("total", total);
        summary.insert("committed", committed);
        summary.insert("best_tests", best_tests);
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_queue() -> (tempfile::TempDir, Queue) {
        let dir = tempfile::tempdir().expect("tempdir");
        let queue = Queue::new(dir.path());
        (dir, queue)
    }

    fn goal(text: &str) -> Goal {
        Goal {
            goal: text.into(),
            dir: None,
            push: false,
            attempts: 0,
        }
    }

    #[test]
    fn an_empty_queue_yields_nothing() {
        let (_dir, queue) = temp_queue();
        assert!(queue.goals().is_empty());
        assert!(queue.next_goal().is_none());
    }

    #[test]
    fn a_goal_round_trips_through_the_file() {
        let (_dir, queue) = temp_queue();
        queue
            .write(&[goal("mach den tui schneller")])
            .expect("write");
        let read = queue.goals();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].goal, "mach den tui schneller");
    }

    #[test]
    fn goals_come_back_oldest_first() {
        let (_dir, queue) = temp_queue();
        queue
            .write(&[goal("erste"), goal("zweite"), goal("dritte")])
            .expect("write");
        let order: Vec<String> = queue.goals().into_iter().map(|g| g.goal).collect();
        assert_eq!(order, vec!["erste", "zweite", "dritte"]);
    }

    #[test]
    fn a_malformed_line_is_skipped_not_fatal() {
        let (_dir, queue) = temp_queue();
        // The queue is written by humans and other agents; one bad line must
        // not take the daemon down.
        std::fs::write(
            queue.path(),
            "{\"goal\":\"gut\"}\nkein json\n{\"goal\":\"auch gut\"}\n",
        )
        .expect("write");
        let read = queue.goals();
        assert_eq!(read.len(), 2, "{read:?}");
    }

    #[test]
    fn a_goal_with_three_failures_is_parked() {
        let (_dir, queue) = temp_queue();
        queue
            .write(&[Goal {
                attempts: 3,
                ..goal("unmoeglich")
            }])
            .expect("write");
        assert!(
            queue.next_goal().is_none(),
            "an unreachable goal must not burn the budget forever"
        );
        // Parked, not deleted — the operator's work is still there.
        assert_eq!(queue.goals().len(), 1);
    }

    #[test]
    fn a_goal_below_the_limit_is_still_worked() {
        let (_dir, queue) = temp_queue();
        queue
            .write(&[
                Goal {
                    attempts: 3,
                    ..goal("geparkt")
                },
                Goal {
                    attempts: 1,
                    ..goal("noch zu retten")
                },
            ])
            .expect("write");
        assert_eq!(
            queue.next_goal().expect("a workable goal").goal,
            "noch zu retten"
        );
    }

    #[test]
    fn a_failure_counts_up_and_the_goal_survives() {
        let (_dir, queue) = temp_queue();
        queue.write(&[goal("schwierig")]).expect("write");
        queue.record_failure(&goal("schwierig")).expect("record");
        let read = queue.goals();
        assert_eq!(read[0].attempts, 1);
        assert_eq!(read[0].goal, "schwierig", "the goal is not deleted");
    }

    #[test]
    fn removing_a_goal_takes_only_that_one() {
        let (_dir, queue) = temp_queue();
        queue
            .write(&[goal("a"), goal("b"), goal("c")])
            .expect("write");
        queue.remove(&goal("b")).expect("remove");
        let left: Vec<String> = queue.goals().into_iter().map(|g| g.goal).collect();
        assert_eq!(left, vec!["a", "c"]);
    }

    #[test]
    fn the_queue_is_written_atomically() {
        let (_dir, queue) = temp_queue();
        queue.write(&[goal("x")]).expect("write");
        // The temp file must not survive the rename — a leftover `.tmp` would
        // tell the next reader a write was interrupted.
        let temporary = queue.path().with_extension("jsonl.tmp");
        assert!(!temporary.exists(), "the temp file outlived the rename");
    }

    #[test]
    fn the_archive_summary_counts_committed_attempts() {
        let dir = tempfile::tempdir().expect("tempdir");
        // An empty archive summarises to zeroes rather than failing: the UI
        // asks for this on a machine that has never evolved anything.
        let summary = Queue::archive_summary(dir.path());
        assert_eq!(summary["total"], 0);
        assert_eq!(summary["committed"], 0);
    }
}
