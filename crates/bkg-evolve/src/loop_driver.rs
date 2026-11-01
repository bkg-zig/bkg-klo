//! The loop itself: read a goal, run a cycle, record what happened.
//!
//! # The shape
//!
//! ```text
//!   ┌─► next_goal() ──► run_cycle() ──► verdict?
//!   │                     │              │
//!   │                     │              ├─ committed → improved++
//!   │                     │              ├─ neutral   → unchanged++, goal done
//!   │                     │              ├─ reverted  → unchanged++, goal done
//!   │                     │              └─ error     → failed++, attempts++, retry
//!   │                     │
//!   └── queue empty ───────┴─► sleep(POLL_INTERVAL) ──┘
//! ```
//!
//! Three rules make this safe to leave running:
//!
//! 1. **One cycle at a time.** Two cycles on one tree would race for the same
//!    branch and the same snapshot store, and the loser would look like a
//!    regression that never happened.
//! 2. **A failing goal is retried, then parked** — three failures, then it
//!    stays in the file untouched. An unreachable goal would otherwise spend a
//!    model budget every thirty seconds with nothing to show.
//! 3. **The stats update even on the failure path.** A daemon whose counters
//!    only move on success looks like it is idle while it burns money.

use std::path::{Path, PathBuf};

use crate::daemon::{DaemonStats, Goal, MAX_GOAL_ATTEMPTS, POLL_INTERVAL, Queue};
use crate::{EvolveOptions, Verdict};

pub use crate::daemon::now_secs;

/// What happened to one goal in one pass.
#[derive(Debug, Clone, PartialEq)]
pub enum Pass {
    /// Nothing queued: the loop should wait.
    Idle,
    /// The cycle committed a better tree.
    Improved { branch: String },
    /// The cycle finished without a regression but without an improvement.
    Unchanged { branch: String, verdict: Verdict },
    /// The cycle could not run; the goal's attempt counter went up.
    Failed { reason: String },
    /// The goal exhausted its attempts and is parked.
    Parked { goal: String },
}

/// One pass over the queue: take at most one goal and work it.
///
/// Returns [`Pass::Idle`] when there is nothing to do, which is the signal for
/// the caller to sleep. Splitting "one pass" from "the loop" is what makes the
/// interesting part testable without a timer or a model.
pub async fn one_pass(queue: &Queue, dir: &Path, options: &EvolveOptions) -> Pass {
    let Some(goal) = queue.next_goal() else {
        return Pass::Idle;
    };

    let work_dir = goal
        .dir
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dir.to_path_buf());

    let pass_options = EvolveOptions {
        push: goal.push || options.push,
        ..options.clone()
    };

    // The goal is announced before the cycle runs, not after. If the daemon
    // dies mid-cycle the UI then shows what it was working on, which is the
    // one question an operator asks after finding the machine busy at 03:00.
    queue.set_current(Some(goal.goal.clone()));
    log_event(
        &crate::daemon::home_root(),
        "work",
        &format!("beginnt: {}", goal.goal),
    );

    match crate::run_cycle(&work_dir, &goal.goal, &pass_options).await {
        Ok(result) => {
            let branch = result.branch.clone();
            // Every finished cycle — whatever the verdict — is a cycle. A
            // counter that only moves on success cannot tell "busy" from
            // "stuck", which is the one distinction the operator needs.
            queue.record_cycle();
            queue.set_current(None);

            match result.verdict {
                Verdict::Improved => {
                    queue.bump(|stats| stats.improved += 1);
                    // An improved goal is done. Leaving it queued would evolve
                    // the same goal forever, re-deriving the same change.
                    let _ = queue.remove(&goal);
                    Pass::Improved { branch }
                }
                // `Suspect` is green but went backwards — tests shrank or
                // warnings grew. It is not an improvement, so it counts with
                // the neutral ones, but the verdict travels with the pass so
                // the UI can say which it was.
                Verdict::Neutral | Verdict::Suspect | Verdict::Reverted => {
                    queue.bump(|stats| stats.unchanged += 1);
                    let _ = queue.remove(&goal);
                    Pass::Unchanged {
                        branch,
                        verdict: result.verdict,
                    }
                }
            }
        }
        Err(reason) => {
            queue.record_cycle();
            queue.bump(|stats| stats.failed += 1);
            queue.set_current(None);

            if let Err(write_error) = queue.record_failure(&goal) {
                // Losing the counter means the goal will be retried forever.
                // That is worse than stopping, so it is reported.
                return Pass::Failed {
                    reason: format!("{reason}; zähler nicht speicherbar: {write_error}"),
                };
            }
            if queue.next_goal().is_none() {
                return Pass::Parked {
                    goal: goal.goal.clone(),
                };
            }
            Pass::Failed { reason }
        }
    }
}

/// Where the running log lives: one appended line per event, newest last.
///
/// # Why a plain text file
///
/// This is the daemon's live channel to a human, and `tail -f` is the one
/// viewer that always exists, always works, and needs no port, no browser and
/// no websocket. Every event appends a line *as it happens*, so the file is
/// the truth about what the daemon is doing — not a snapshot someone has to
/// remember to look at.
///
/// It is deliberately not the journal. The journal is regenerated from the
/// attempt archive after every cycle and holds what was *learned*; this holds
/// what is *happening*. Mixing them would make the journal rewrite itself
/// under a tail, and the operator would see lines disappear.
pub fn changelog_path(home: &Path) -> PathBuf {
    home.join("evolve").join("changelog.log")
}

/// Append one line to the running log.
///
/// Best-effort by design: a daemon whose log cannot be written should keep
/// evolving, not stop. That is the difference between a log and a gate — and
/// this one is a log.
pub fn log_event(home: &Path, level: &str, message: &str) {
    let path = changelog_path(home);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let stamp = crate::daemon::now_secs();
    let line = format!("{stamp} {level:<7} {message}\n");
    // `OpenOptions::append` rather than `write`: an append from two processes
    // cannot truncate what the other wrote.
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path);
    if let Ok(mut file) = opened {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

/// The tail of the running log, newest last, for the UI.
pub fn changelog_tail(home: &Path, lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(changelog_path(home)) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all.iter()
        .skip(all.len().saturating_sub(lines))
        .map(|line| line.to_string())
        .collect()
}

/// Start the daemon and never return.
///
/// The name is literal. A caller that wants a bounded number of passes uses
/// [`one_pass`] directly — that is the whole reason it is separate.
pub async fn run_daemon(dir: &Path, options: EvolveOptions) -> Result<(), String> {
    let home = bkgclaw_store::home_root();
    let queue = Queue::new(&home);
    queue.bump(|stats| stats.started_ts = crate::daemon::now_secs());

    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "`{}` ist kein cargo-workspace — evolve hat nichts zu tun",
            dir.display()
        ));
    }
    // No remote means improvements accumulate on branches nobody sees. The
    // UI shows this, because silence about it is the expensive kind. The
    // options' `origin` is one way to configure one, but a tree cloned from
    // a checkout already carries its `origin` — checking only the options
    // would warn about a remote that works.
    let push_ready = options.origin.is_some()
        || crate::gitops::remote_url(dir, "origin").is_some_and(|url| !url.trim().is_empty());
    if !push_ready {
        queue.bump(|stats| stats.origin_missing = true);
    }

    log_event(
        &home,
        "start",
        &format!("daemon startet in {}", dir.display()),
    );
    if !push_ready {
        log_event(
            &home,
            "warn",
            "kein push-remote — verbesserungen bleiben lokal",
        );
    }

    loop {
        let pass = one_pass(&queue, dir, &options).await;
        match &pass {
            Pass::Improved { branch } => {
                log_event(&home, "better", &format!("verbessert: {branch}"));
            }
            Pass::Unchanged { branch, verdict } => {
                log_event(
                    &home,
                    "same",
                    &format!("{verdict:?} — {branch} bleibt liegen"),
                );
            }
            Pass::Failed { reason } => {
                log_event(&home, "fail", reason);
                eprintln!("evolve: {reason}");
            }
            Pass::Parked { goal } => {
                log_event(
                    &home,
                    "park",
                    &format!("geparkt nach {MAX_GOAL_ATTEMPTS} versuchen: {goal}"),
                );
            }
            Pass::Idle => {}
        }
        if matches!(pass, Pass::Idle) {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

/// A summary for the UI, read straight from the queue's counters.
pub fn stats_for(home: &Path) -> (DaemonStats, std::collections::HashMap<&'static str, u64>) {
    let queue = Queue::new(home);
    let mut stats = queue.stats();
    if stats.started_ts == 0 {
        stats.started_ts = crate::daemon::now_secs();
    }
    let summary = Queue::archive_summary(home);
    (stats, summary)
}

/// Seed the queue with one goal — the daemon's first run has nothing to do
/// otherwise, and "it does nothing" is indistinguishable from "it is broken".
pub fn seed(home: &Path, goal: &str) -> Result<(), String> {
    let queue = Queue::new(home);
    let mut goals = queue.goals();
    if goals.iter().any(|entry| entry.goal == goal) {
        return Ok(());
    }
    goals.push(Goal {
        goal: goal.to_string(),
        dir: None,
        push: false,
        attempts: 0,
    });
    queue.write(&goals)
}

/// Where the queue lives, for the UI to show.
pub fn queue_path(home: &Path) -> std::path::PathBuf {
    Queue::new(home).path().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::daemon::MAX_GOAL_ATTEMPTS;

    #[test]
    fn seeding_twice_does_not_duplicate_a_goal() {
        // Running `evolve seed` twice must not queue the same work twice —
        // that would evolve it twice for no reason.
        let dir = tempfile::tempdir().expect("tempdir");
        seed(dir.path(), "mach den tui schneller").expect("seed");
        seed(dir.path(), "mach den tui schneller").expect("seed");
        assert_eq!(Queue::new(dir.path()).goals().len(), 1);
    }

    #[test]
    fn seeding_is_visible_to_the_next_reader() {
        let dir = tempfile::tempdir().expect("tempdir");
        seed(dir.path(), "ziel").expect("seed");
        let queue = Queue::new(dir.path());
        assert_eq!(queue.next_goal().expect("queued").goal, "ziel");
    }

    #[test]
    fn the_stats_summary_works_on_an_untouched_machine() {
        // The UI asks for stats on boot; a machine that never evolved must
        // still get an answer rather than an error.
        let dir = tempfile::tempdir().expect("tempdir");
        let (stats, summary) = stats_for(dir.path());
        assert_eq!(stats.cycles, 0);
        assert_eq!(summary["total"], 0);
    }

    #[test]
    fn an_idle_pass_is_the_signal_to_wait() {
        let dir = tempfile::tempdir().expect("tempdir");
        let queue = Queue::new(dir.path());
        let options = EvolveOptions::default();
        // The pass is async but the idle path does no work, so a blocking
        // runtime is enough and keeps the test free of a timer.
        let pass = futures_executor::block_on(one_pass(&queue, dir.path(), &options));
        assert_eq!(pass, Pass::Idle);
    }

    #[test]
    fn the_poll_interval_is_long_enough_not_to_burn_budget() {
        // Thirty seconds is the difference between a daemon and a money printer.
        assert!(
            POLL_INTERVAL >= Duration::from_secs(10),
            "{POLL_INTERVAL:?}"
        );
    }

    #[test]
    fn the_changelog_appends_and_never_truncates() {
        // The whole point is `tail -f`: a line that overwrites its predecessor
        // would make the log unreadable exactly when someone is watching it.
        let dir = tempfile::tempdir().expect("tempdir");
        log_event(dir.path(), "start", "erste zeile");
        log_event(dir.path(), "work", "zweite zeile");
        log_event(dir.path(), "better", "dritte zeile");

        let text = std::fs::read_to_string(changelog_path(dir.path())).expect("changelog");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "three lines survived: {text}");
        assert!(lines[0].contains("start"), "{}", lines[0]);
        assert!(lines[1].contains("zweite zeile"), "{}", lines[1]);
        assert!(lines[2].contains("dritte zeile"), "{}", lines[2]);
    }

    #[test]
    fn every_changelog_line_is_timestamped_and_levelled() {
        // A line without a timestamp cannot be ordered against the others, and
        // one without a level cannot be filtered. Both are what a tail is for.
        let dir = tempfile::tempdir().expect("tempdir");
        log_event(dir.path(), "fail", "etwas ging schief");
        log_event(dir.path(), "better", "trotzdem besser");

        let tail = changelog_tail(dir.path(), 10);
        assert_eq!(tail.len(), 2);
        for line in &tail {
            let stamp: u64 = line
                .split_whitespace()
                .next()
                .and_then(|first| first.parse().ok())
                .unwrap_or_else(|| panic!("kein zeitstempel in {line:?}"));
            assert!(stamp > 1_600_000_000, "plausibler Zeitstempel: {line}");
        }
        assert!(tail[0].contains("fail"), "{}", tail[0]);
        assert!(tail[1].contains("better"), "{}", tail[1]);
    }

    #[test]
    fn the_tail_returns_the_newest_lines() {
        // The panel shows the end of the log, not the beginning — the newest
        // event is the one an operator is waiting for.
        let dir = tempfile::tempdir().expect("tempdir");
        for index in 0..50 {
            log_event(dir.path(), "work", &format!("schritt {index}"));
        }
        let tail = changelog_tail(dir.path(), 5);
        assert_eq!(tail.len(), 5);
        assert!(tail[4].contains("schritt 49"), "{}", tail[4]);
    }

    #[test]
    fn a_missing_changelog_is_an_empty_tail_not_a_failure() {
        // The panel opens on a machine that has never evolved. An empty log is
        // the state, not an error to report.
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(changelog_tail(dir.path(), 10).is_empty());
    }

    #[test]
    fn logging_survives_an_unwritable_directory() {
        // A daemon whose log cannot be written must keep evolving. The log is
        // a channel to a human, not a gate on the loop.
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("evolve");
        std::fs::write(&blocker, "kein verzeichnis").expect("blocker");
        log_event(dir.path(), "work", "wird verworfen, aber nicht zum fehler");
    }

    #[test]
    fn parked_goals_are_not_lost() {
        let dir = tempfile::tempdir().expect("tempdir");
        let queue = Queue::new(dir.path());
        queue
            .write(&[crate::daemon::Goal {
                attempts: MAX_GOAL_ATTEMPTS,
                ..crate::daemon::Goal {
                    goal: "unmoeglich".into(),
                    dir: None,
                    push: false,
                    attempts: 0,
                }
            }])
            .expect("write");
        assert!(queue.next_goal().is_none());
        assert_eq!(
            queue.goals().len(),
            1,
            "a parked goal stays visible to the operator"
        );
    }
}
