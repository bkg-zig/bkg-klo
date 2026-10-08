//! The evolution engine: one full cycle, safely.
//!
//! ```text
//! bkgclaw evolve run "mache die tui-suche schneller"
//!   1. GUARD      git init (idempotent) · branch evolve/NNN-slug · snapshot
//!   2. BRIEF      coding-prompt + goal + fitness-Kriterien + recente lehren
//!   3. WORK       ein agenten-turn über den NIM — der agent ändert den code,
//!                 darf cargo selbst laufen, darf NICHT committen
//!   4. JUDGE      fitness: echte cargo build/test/clippy exit-codes
//!   5. DECIDE     improved/neutral/suspect → commit auf dem branch
//!                 reverted → hard reset + snapshot-rollback
//!   6. RECORD     attempt-zeile + lehre + journal regeneration
//!   7. PUSH       (optional) branch + journal nach origin
//! ```
//!
//! Warum das besser ist als ein naiver "read source, change, commit"-Loop:
//! das Urteil fällt **nie** über die Worte des Agenten, sondern über die
//! Ausgabecodes seiner Werkzeuge; gescheiterte Versuche hinterlassen
//! Lehren, die der nächste Versuch im Prompt sieht; und der Hauptzweig
//! wird von keinem Modell direkt angerührt.

use std::path::{Path, PathBuf};

use bkgclaw_core::loop_engine::{run_agent_turns_with, StopReason};
use bkgclaw_core::models::Message;
use bkgclaw_core::tools::Policy;
use bkgclaw_core::{Budget, Detector, LoopConfig};

use crate::fitness::{self, FitnessReport, Verdict};
use crate::gitops;
use crate::memory::{lesson_for, Attempt, AttemptArchive};

/// What one cycle should do differently.
#[derive(Debug, Clone)]
pub struct EvolveOptions {
    /// Push the attempt branch (and main if it moved) after recording.
    pub push: bool,
    /// Origin URL for `init` and first push; remembered in git afterwards.
    pub origin: Option<String>,
    /// How many recent lessons ride along in the briefing.
    pub lesson_count: usize,
    /// Commit only on a measurably better tree; neutral results stay on
    /// the branch uncommitted, suspect results are reverted like failures.
    pub strict: bool,
    /// Run the WORK phase inside the Landlock write-jail: the agent may
    /// write only this tree. Fail-closed — a refused jail refuses the
    /// cycle, an unsandboxed work phase is not an option.
    ///
    /// Defaults to **true**. This is the one switch that decides whether an
    /// unattended daemon is safe: with the jail, a worker's `rm -rf` reaches
    /// this repository and nothing else; without it, `Policy::AllowAll` is an
    /// open door. Leaving this off by default would have meant every daemon
    /// anyone started was running wide open until somebody noticed.
    pub jail: bool,
}

impl Default for EvolveOptions {
    fn default() -> Self {
        EvolveOptions {
            push: false,
            origin: None,
            lesson_count: 5,
            strict: false,
            jail: default_true(),
        }
    }
}

/// Serde's idea of "this field was absent" is a default, and for a safety
/// switch the safe answer is the only sensible one.
fn default_true() -> bool {
    true
}

/// The outcome the caller (CLI, gateway, UI) reports.
#[derive(Debug, Clone)]
pub struct CycleResult {
    pub id: u64,
    pub branch: String,
    pub goal: String,
    pub verdict: Verdict,
    pub fitness: FitnessReport,
    pub baseline: FitnessReport,
    pub lesson: String,
    pub answer: String,
    pub stop_reason: StopReason,
    pub pushed: bool,
}

/// A slug for branch names: goal text to filesystem-safe.
pub fn slug(goal: &str) -> String {
    let mut slug: String = goal
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        slug = "versuch".into();
    }
    slug.chars().take(40).collect()
}

/// The system prompt for a self-improvement turn. Sharper than the normal
/// coding prompt, because the stakes are different: the agent is editing
/// the code that runs it.
fn evolve_prompt(dir: &Path, goal: &str, lessons: &[String]) -> String {
    let mut prompt = format!(
        "\
Du bist bkgclaw, gerade dabei, DEINEN EIGENEN Quellcode zu verbessern.

Arbeitsverzeichnis: {dir}
Auftrag: {goal}

Das Urteil über deine Arbeit fällt NICHT du — es fallen cargo build, cargo test und
cargo clippy über echte Exit-Codes. Behauptungen zählen nicht, Messungen zählen.

Regeln für diesen Lauf:
- Du darfst lesen, schreiben, bearbeiten und cargo über execute_command selbst laufen
  lassen. Nutze das: lauf die Tests, BEVOR du fertig meldest.
- Du darfst KEINE git-Befehle ausführen. Branches, Commits und Rollbacks macht das
  evolve-system nach der Bewertung. Wer selbst committed, schreibt sein eigenes Zeugnis.
- Halte die Änderung klein und zusammenhängend: ein Auftrag, eine Änderung.
- Wenn dir die Lehren unten etwas über frühere Fehlversuche sagen: wiederhole deren
  Fehler nicht.",
        dir = dir.display(),
        goal = goal,
    );
    if !lessons.is_empty() {
        prompt.push_str("\n\nFrühere Versuche (neueste zuletzt):\n");
        prompt.push_str(&lessons.join("\n"));
    }
    prompt
}

/// Branches that evolution may never check out or reset.
///
/// The reason is not politeness. In this repository the gateway daemon runs
/// from this working directory, so `git reset --hard` on the branch it is
/// served from is not a code change — it is an outage. Evolution works on a
/// branch of its own and comes back to the operator to merge.
///
/// `BKGCLAW_PROTECTED_BRANCHES` extends the list, because a deployment might
/// run the server from something other than `main`.
fn protected_branches(_dir: &Path) -> Vec<String> {
    let mut names = vec!["main".to_string(), "master".to_string()];
    if let Ok(extra) = std::env::var("BKGCLAW_PROTECTED_BRANCHES") {
        names.extend(
            extra
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string),
        );
    }
    names
}

/// Measure the baseline before the agent starts, on the clean tree.
fn baseline_fitness(dir: &Path) -> FitnessReport {
    fitness::measure(dir)
}

/// What the WORK phase produced — evidence for the record, never the
/// verdict. The stop reason says how the turn ended ("Answered" after a
/// single no-tool turn is the honest signature of a model that talked
/// instead of worked).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkOutcome {
    #[serde(default)]
    pub answer: String,
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default)]
    pub turns: u32,
}

/// Phase 3 on its own: the only phase where the model's hands touch the
/// tree. Public + separate so the daemon can run exactly this part
/// inside the sandbox jail — guard, judge and record stay outside, in
/// our code. One agent turn, full toolset (it must run its own tests),
/// the fitness gate judges afterwards.
pub async fn work_phase(dir: &Path, goal: &str, lessons: &[String]) -> Result<WorkOutcome, String> {
    let registry = bkgclaw_core::wiring::model_registry();
    let chain = bkgclaw_core::wiring::default_chain()
        .await
        .into_iter()
        .filter(|candidate| registry.for_vendor(candidate.model.vendor).is_some())
        .collect::<Vec<_>>();
    if chain.is_empty() {
        return Err("kein nutzbares modell — NIM_API_KEY setzen oder ollama starten".into());
    }

    let executor = bkgclaw_exec::LocalExecutor::new();
    let config = LoopConfig {
        max_turns: bkgclaw_core::loop_engine::max_turns(),
        max_tool_output: 12_000,
        policy: Policy::AllowAll,
        overrides: Default::default(),
        detector: Detector::new(),
    };
    let transcript = vec![
        Message::system(evolve_prompt(dir, goal, lessons)),
        Message::user(format!("Verbessere jetzt: {goal}. Lauf die Tests, bevor du fertig meldest.")),
    ];

    let mut router = bkgclaw_core::Router::new(&registry, chain);
    router.breakers = bkgclaw_core::wiring::default_breakers();
    let mut budget = Budget::new(Some(2.0));

    let run = run_agent_turns_with(
        &mut router,
        &bkgclaw_core::tools::builtin_tools(),
        &executor,
        &config,
        &mut budget,
        transcript,
        &bkgclaw_core::NoObserver,
    )
    .await;

    let answer = run
        .turns
        .iter()
        .rev()
        .map(|t| t.text.as_str())
        .find(|t| !t.is_empty())
        .unwrap_or("")
        .to_string();
    Ok(WorkOutcome {
        answer,
        stop_reason: format!("{:?}", run.stop_reason),
        turns: run.turns.len() as u32,
    })
}

/// The handshake files between the daemon and its jailed worker, both
/// inside `.bkgclaw/` — gitignored, so the tree stays clean for the
/// fitness gate and the attempt commit.
fn work_handshake(dir: &Path) -> (PathBuf, PathBuf) {
    let base = dir.join(".bkgclaw");
    (base.join("evolve-work-in.json"), base.join("evolve-work-out.json"))
}

/// Phase 3 inside the Landlock write-jail. The jailed process is this
/// binary running the hidden `evolve work` subcommand: the briefing
/// rides in through a file, the outcome rides back the same way, and
/// the agent can write nothing outside this tree. Fail-closed — a
/// refused jail refuses the cycle; an unsandboxed work phase is not an
/// option the daemon ever takes.
async fn jailed_work_phase(dir: &Path, goal: &str, lessons: &[String]) -> Result<WorkOutcome, String> {
    let (in_path, out_path) = work_handshake(dir);
    if let Some(parent) = in_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("handshake-verzeichnis: {e}"))?;
    }
    let briefing = serde_json::json!({ "goal": goal, "lessons": lessons });
    std::fs::write(&in_path, briefing.to_string()).map_err(|e| format!("handshake-in: {e}"))?;
    let _ = std::fs::remove_file(&out_path);

    let dir_owned = dir.to_path_buf();
    let in_path_rm = in_path.clone();
    // `spawn_blocking` returns `JoinHandle<T>`; awaiting it yields `T`, never a
    // `Result` — so there is no `?` to write. The `JoinError` case is the
    // panic case, which the map handles; the closure's own `JailOutcome` is the
    // value that comes back.
    let handle = tokio::task::spawn_blocking(move || {
        let exe = std::env::current_exe()
            .map_err(|e| format!("eigene binary nicht gefunden: {e}"))?;
        let config = bkgclaw_sandbox::Config::default(); // write: ["."], read: ["/"]
        let argv = vec![exe.display().to_string(), "evolve".into(), "work".into()];
        let outcome = bkgclaw_sandbox::jail::run(&config, &dir_owned, &argv);
        let _ = std::fs::remove_file(&in_path_rm);
        Ok::<_, String>(outcome)
    })
    .await
    .map_err(|e| format!("jail-task: {e}"))?;

    match handle {
        Ok(bkgclaw_sandbox::JailOutcome::Ran(0)) => {}
        Ok(bkgclaw_sandbox::JailOutcome::Ran(code)) => {
            let _ = std::fs::remove_file(&out_path);
            return Err(format!("work-phase im jail endete mit exit {code}"));
        }
        Ok(bkgclaw_sandbox::JailOutcome::Refused(why)) => {
            let _ = std::fs::remove_file(&out_path);
            return Err(format!("jail verweigert — kein unsandboxer versuch: {why}"));
        }
        Err(why) => {
            let _ = std::fs::remove_file(&out_path);
            return Err(why);
        }
    }
    let outcome: WorkOutcome = std::fs::read_to_string(&out_path)
        .map_err(|e| format!("keine work-antwort vom jailed prozess: {e}"))
        .and_then(|text| {
            let _ = std::fs::remove_file(&out_path);
            serde_json::from_str(&text).map_err(|e| format!("work-antwort keine json: {e}"))
        })?;
    Ok(outcome)
}

/// Run one complete evolution cycle.
pub async fn run_cycle(dir: &Path, goal: &str, options: &EvolveOptions) -> Result<CycleResult, String> {
    if goal.trim().is_empty() {
        return Err("ein evolve-lauf braucht ein ziel: bkgclaw evolve run \"…\"".into());
    }

    let home = bkgclaw_store::home_root();
    let archive = AttemptArchive::new(&home);

    // 0 — PROTECT THE RUNNING TREE.
    //
    // A cycle ends in `git reset --hard` on failure and in a commit on
    // success. On `main` that is not an edit, it is an outage: the gateway
    // runs from this very directory, so a revert during an unattended run
    // takes the server down. Evolution therefore gets its own branch and
    // never touches the one something is running from.
    let protected = protected_branches(dir);
    let current = gitops::current_branch(dir).unwrap_or_default();
    if protected.iter().any(|name| name == &current) {
        return Err(format!(
            "evolve läuft auf `{}` — geschützt: {}. \
             der server läuft aus diesem baum; ein `git reset --hard` wäre ein ausfall. \
             evolution auf einen eigenen zweig: `git switch -c evolve`",
            current,
            protected.join(", "),
        ));
    }

    // 1 — GUARD: repo, clean tree, branch, snapshot. The branch exists
    // even on failure; forensics beats tidiness — but a dirty tree
    // cannot be forensics: a commit would sweep the operator's half-
    // finished work into the attempt and misattribute it forever.
    gitops::init(dir)?;
    let dirty = gitops::dirty_files(dir);
    if !dirty.is_empty() {
        return Err(format!(
            "der baum ist nicht sauber ({} datei(en), z. b. `{}`) — erst committen, dann evolven.              ein evolve-commit würde fremde arbeit dem versuch zuschreiben.",
            dirty.len(),
            dirty.first().map(String::as_str).unwrap_or("?"),
        ));
    }
    if let Some(origin) = &options.origin {
        gitops::remote_add_origin(dir, origin)?;
    }
    let id = archive.next_id();
    let branch = format!("evolve/{id:04}-{}", slug(goal));
    gitops::branch(dir, &branch)?;

    // Baseline before anything changes — "still green" is not "better".
    let baseline = baseline_fitness(dir);

    // Snapshot for the double rollback: git resets the tracked tree, the
    // snapshot store restores everything else the versions system knows.
    let snapshot_id = bkgclaw_store::auto_snapshot(&home, dir, goal);

    // 2 — BRIEF: lessons ride along.
    let lessons = archive.briefing_lines(options.lesson_count);

    // 3 — WORK: one agent turn with full write access — the fitness gate
    // is the control, and approvals have no human awake at 3 a.m.
    let work = if options.jail {
        jailed_work_phase(dir, goal, &lessons).await?
    } else {
        work_phase(dir, goal, &lessons).await?
    };
    let answer = work.answer;

    // 4 — JUDGE: nobody asks the agent how it went.
    let report = fitness::measure(dir);
    let verdict = fitness::verdict(&report, &baseline);

    // 5 — DECIDE.
    let lesson = lesson_for(goal, verdict, &report);
    let mut pushed = false;
    let commits = match verdict {
        Verdict::Improved => true,
        // Strict mode demands measurable improvement; neutral adds noise.
        Verdict::Neutral => !options.strict,
        // Suspect never lands on the branch: tests shrank or lint grew.
        Verdict::Suspect => false,
        Verdict::Reverted => false,
    };
    match if commits { verdict } else { Verdict::Reverted } {
        Verdict::Improved | Verdict::Neutral | Verdict::Suspect => {
            // Green (with or without an asterisk): commit on the branch.
            gitops::add_all(dir)?;
            let message = format!(
                "evolve {id:04}: {goal} [{verdict} — {passed}/{total} tests, {clippy} warnungen]",
                verdict = verdict.as_str(),
                passed = report.tests_passed,
                total = report.tests_passed + report.tests_failed,
                clippy = report.clippy_warnings,
            );
            let _commit = gitops::commit(dir, &message);
            if options.push {
                // The branch tells the story; main follows only when the
                // operator merges it deliberately.
                pushed = gitops::push(dir, "origin", &branch, true).is_ok();
            }
        }
        Verdict::Reverted => {
            // The attempt failed its own tests: the tree goes back.
            // Git resets the tracked files, the snapshot restores the
            // rest — between them, the working tree is the baseline again.
            let _ = gitops::hard_reset(dir);
            if let Some(snapshot_id) = &snapshot_id {
                let store = bkgclaw_store::SnapshotStore::for_dir(&home, dir);
                let _ = store.restore(dir, snapshot_id);
            }
            // Back to main; the attempt branch keeps the corpse.
            let _ = gitops::checkout(dir, "main");
        }
    }

    // 6 — RECORD.
    let attempt = Attempt {
        id,
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        goal: goal.to_string(),
        branch: branch.clone(),
        verdict,
        fitness: report.clone(),
        lesson: lesson.clone(),
        origin: options.origin.clone().unwrap_or_default(),
        stop_reason: work.stop_reason.clone(),
        turns: work.turns,
        answer: answer.clone(),
    };
    archive.record(attempt)?;

    Ok(CycleResult {
        id,
        branch,
        goal: goal.to_string(),
        verdict,
        fitness: report,
        baseline,
        lesson,
        answer,
        stop_reason: parse_stop_reason(&work.stop_reason),
        pushed,
    })
}

/// Turn a work phase's textual reason into a typed one.
///
/// The work phase crosses a process boundary — it runs as `bkgclaw evolve
/// work` inside the jail and answers with JSON — so its `stop_reason` arrives
/// as a string. `CycleResult` wants the enum, because the archive and the
/// fitness report branch on it.
///
/// An unknown string is `Answered`, not a panic: the work phase can report
/// reasons this crate has not been taught yet, and a newer worker must not
/// crash an older reader. That trade is stated rather than hidden — a new
/// reason shows up as "answered" rather than as itself.
fn parse_stop_reason(raw: &str) -> StopReason {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "answered" | "answer" => StopReason::Answered,
        "turnlimit" | "turn_limit" | "max_turns" => StopReason::TurnLimit,
        "budgetblocked" | "budget_blocked" => StopReason::BudgetBlocked,
        "cancelled" | "canceled" => StopReason::Cancelled,
        other => {
            // `ProviderFailed` and `LoopDetected` carry a payload, and the
            // string *is* that payload. Anything else with a colon is
            // `name: detail`, which is how the work phase writes them.
            match other.split_once(':') {
                Some(("providerfailed" | "provider_failed" | "provider", detail)) => {
                    StopReason::ProviderFailed(detail.trim().to_string())
                }
                Some(("loopdetected" | "loop_detected", detail)) => {
                    StopReason::LoopDetected(detail.trim().to_string())
                }
                _ => StopReason::Answered,
            }
        }
    }
}

/// Initialize the evolution repo: git init, ignore file, first commit,
/// optional origin. This is `bkgclaw evolve init`.
pub fn init_repo(dir: &Path, origin: Option<&str>) -> Result<(), String> {
    gitops::init(dir)?;
    if let Some(origin) = origin {
        gitops::remote_add_origin(dir, origin)?;
    }
    let dirty = gitops::dirty_files(dir);
    if !dirty.is_empty() {
        gitops::add_all(dir)?;
        gitops::commit(dir, "evolve 0000: kernel-baseline — bkgclaw verwaltet ab hier sein eigenes git")?;
    } else if gitops::commit(dir, "evolve 0000: kernel-baseline — leerer start").is_err() {
        // Nothing to commit and nothing to say: a fresh repo with nothing
        // in it still needs a root commit for branches to stand on.
        return Err("baseline-commit fehlgeschlagen".into());
    }
    Ok(())
}

/// Push the current branch and main — the operator's explicit act, or
/// the automatic one after an improved attempt.
pub fn push_state(dir: &Path) -> Result<(), String> {
    let branch = gitops::current_branch(dir)?;
    gitops::push(dir, "origin", &branch, true)?;
    Ok(())
}

/// The journal path for UIs to read.
pub fn journal_path() -> PathBuf {
    bkgclaw_store::home_root().join("evolve").join("journal.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_become_filesystem_safe_slugs() {
        assert_eq!(slug("Mache die TUI schneller!"), "Mache-die-TUI-schneller");
        assert_eq!(slug("///"), "versuch");
        // Nicht-ASCII wird zu '-' kollabiert — lesbar und dateisystem-sicher.
        assert!(slug("äöü").starts_with("versuch"), "{}", slug("äöü"));
        let long = "x".repeat(100);
        assert!(slug(&long).len() <= 40);
    }

    #[test]
    fn the_prompt_names_the_rules_that_matter() {
        let lessons = vec!["- versuch 1 (reverted, 10/2 tests): tests vergessen".to_string()];
        let prompt = evolve_prompt(Path::new("/repo"), "lesegeschwindigkeit", &lessons);
        assert!(prompt.contains("Exit-Codes"), "das urteil muss exit-codes heißen");
        assert!(prompt.contains("KEINE git-Befehle"), "der agent committet nicht selbst");
        assert!(prompt.contains("lesegeschwindigkeit"));
        assert!(prompt.contains("tests vergessen"), "lehren müssen mitreisen");
    }

    #[test]
    fn risk_classes_are_untouched_by_evolve() {
        // Sanity anchor: the evolve engine grants AllowAll for its OWN
        // gated run, but the registry's classification of destructive
        // tools stays intact for every other caller.
        let registry = bkgclaw_core::tools::builtin_tools();
        assert_eq!(
            registry
                .gate("execute_command", Policy::AllowReadOnly, &Default::default())
                .decision,
            bkgclaw_core::tools::Decision::Deny
        );
        let _ = bkgclaw_core::tools::Risk::Destructive;
    }
}

#[cfg(test)]
mod clean_tree_tests {
    use super::*;

    #[test]
    fn a_dirty_tree_refuses_to_evolve() {
        let dir = tempfile::tempdir().unwrap().keep();
        // Dirty: init creates the baseline commit, then we add a file.
        gitops::init(&dir).unwrap();
        std::fs::write(dir.join("halbe-arbeit.txt"), "nicht meins").unwrap();
        // run_cycle needs a model — but the guard fires before any model
        // is touched, so a bare call must fail on dirtiness, not on
        // "kein modell". (tokio not available here: the guard is
        // synchronous at the top of run_cycle, so a spawned runtime
        // still fails before the registry is built.)
        let outcome = {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("test-runtime");
            rt.block_on(async { run_cycle(&dir, "test", &EvolveOptions::default()).await })
        };
        let error = outcome.unwrap_err();
        assert!(error.contains("nicht sauber"), "{error}");
        assert!(error.contains("datei(en)"), "die meldung zählt: {error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
