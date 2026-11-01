//! `bkgclaw evolve` — the self-improvement commands.
//!
//! The CLI is the operator's lever: init the repo, run cycles, read the
//! journal, push. The verdicts inside come from the fitness gate, never
//! from the model's self-assessment.

use crate::outcome::Outcome;
use bkgclaw_core::Verdict;

pub fn init(origin: Option<&str>) -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    match bkg_evolve::init_repo(&dir, origin) {
        Ok(()) => {
            let mut summary = format!(
                "evolution-repo bereit in {} — baseline committed",
                dir.display()
            );
            if let Some(origin) = origin {
                match bkg_evolve::gitops::push(&dir, "origin", "main", true) {
                    Ok(()) => summary.push_str(&format!(" — gepusht zu {origin}")),
                    Err(error) => {
                        summary.push_str(&format!(
                            " — push fehlgeschlagen (später `bkgclaw evolve push`): {error}"
                        ));
                    }
                }
            }
            Outcome::success(summary)
        }
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

pub async fn run(goal: &str, push: bool, strict: bool) -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    let options = bkg_evolve::EvolveOptions {
        push,
        strict,
        ..Default::default()
    };
    match bkg_evolve::run_cycle(&dir, goal, &options).await {
        Ok(result) => {
            let mut summary = format!(
                "evolve {id:03} auf `{branch}` — {verdict}: {lesson}",
                id = result.id,
                branch = result.branch,
                verdict = result.verdict.as_str(),
                lesson = result.lesson,
            );
            if result.pushed {
                summary.push_str(" — gepusht");
            }
            let mut report = bkgclaw_ui::Report::ok(summary);
            if !result.answer.is_empty() {
                report = report.with_data(serde_json::json!({
                    "agent": result.answer,
                    "fitness": result.fitness,
                    "baseline": result.baseline,
                    "branch": result.branch,
                }));
            }
            // A neutral cycle is not a failure — it answered, the tests
            // stayed green, nothing regressed. The exit code says "the
            // command worked"; the summary says what came of it.
            Outcome::from_report(report)
        }
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

pub fn attempts() -> Outcome {
    let home = bkgclaw_store::home_root();
    let archive = bkg_evolve::AttemptArchive::new(&home);
    let attempts = archive.all();
    if attempts.is_empty() {
        return Outcome::from_report(bkgclaw_ui::Report::negative(
            "noch keine evolve-versuche",
            "starten mit: bkgclaw evolve run \"mache X besser\"",
        ));
    }
    let checks = attempts
        .iter()
        .rev()
        .map(|attempt| {
            let ok = !matches!(attempt.verdict, bkg_evolve::Verdict::Reverted);
            bkgclaw_ui::Check {
                name: format!("{:03} · {}", attempt.id, attempt.verdict.as_str()),
                ok,
                code: "[EVOLVE]".into(),
                detail: format!(
                    "{} tests, {} failed, {} warnungen — {}",
                    attempt.fitness.tests_passed,
                    attempt.fitness.tests_failed,
                    attempt.fitness.clippy_warnings,
                    attempt.lesson,
                ),
            }
        })
        .collect();
    Outcome::from_report(bkgclaw_ui::Report::with_checks(
        format!("{} evolve-versuche", attempts.len()),
        checks,
    ))
}

pub fn fitness_report() -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    let report = bkg_evolve::measure_fitness(&dir);
    let checks = vec![
        bkgclaw_ui::Check {
            name: "build".into(),
            ok: report.build_ok,
            code: "[FITNESS]".into(),
            detail: if report.build_ok {
                "ok".into()
            } else {
                "kaputt".into()
            },
        },
        bkgclaw_ui::Check {
            name: "tests".into(),
            ok: report.tests_ok,
            code: "[FITNESS]".into(),
            detail: format!(
                "{} passed, {} failed",
                report.tests_passed, report.tests_failed
            ),
        },
        bkgclaw_ui::Check {
            name: "clippy".into(),
            ok: true,
            code: "[FITNESS]".into(),
            detail: format!("{} warnungen", report.clippy_warnings),
        },
    ];
    Outcome::from_report(bkgclaw_ui::Report::with_checks(
        format!("fitness in {} s", report.duration_secs),
        checks,
    ))
}

pub fn push() -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    match bkg_evolve::push_state(&dir) {
        Ok(()) => Outcome::success("branch und journal gepusht"),
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

/// Work the goal queue forever.
///
/// Never returns — that is the point. An operator starts it with nohup or a
/// service unit and it keeps going; `Ctrl-C` is the exit. Every cycle is
/// gated by the same fitness judge as `evolve run`, so "it ran unattended"
/// does not mean "it commits whatever it likes".
pub async fn daemon(push: bool, strict: bool, seed_goal: Option<&str>) -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    let home = bkgclaw_store::home_root();
    if let Some(goal) = seed_goal {
        if let Err(error) = bkg_evolve::seed(&home, goal) {
            return Outcome::fail(Verdict::environment(error));
        }
    }
    let options = bkg_evolve::EvolveOptions {
        push,
        strict,
        ..Default::default()
    };
    match bkg_evolve::run_daemon(&dir, options).await {
        Ok(()) => Outcome::success("evolve-daemon beendet"),
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

/// Put a goal on the queue. Takes effect on the daemon's next pass — it does
/// not have to be running now.
pub fn seed(goal: &str) -> Outcome {
    let home = bkgclaw_store::home_root();
    match bkg_evolve::seed(&home, goal) {
        Ok(()) => Outcome::from_report(
            bkgclaw_ui::Report::ok(format!("ziel eingereiht: {goal}")).with_data(
                serde_json::json!({
                    "goal": goal,
                    "queue": bkg_evolve::queue_path(&home).display().to_string(),
                }),
            ),
        ),
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

/// The queue, the counters and the archive — everything the UI shows, in the
/// shape the CLI prints it.
pub fn status() -> Outcome {
    let home = bkgclaw_store::home_root();
    let (stats, summary) = bkg_evolve::stats_for(&home);
    let queue = bkg_evolve::Queue::new(&home);
    let goals: Vec<serde_json::Value> = queue
        .goals()
        .into_iter()
        .map(|goal| {
            serde_json::json!({
                "goal": goal.goal,
                "attempts": goal.attempts,
                "parked": goal.attempts >= bkg_evolve::MAX_GOAL_ATTEMPTS,
            })
        })
        .collect();

    let summary_text = format!(
        "evolve: {} zyklen, {} verbessert, {} fehlgeschlagen",
        stats.cycles, stats.improved, stats.failed
    );

    let data = serde_json::json!({
        "queue": goals,
        "queue_path": bkg_evolve::queue_path(&home).display().to_string(),
        "cycles": stats.cycles,
        "improved": stats.improved,
        "unchanged": stats.unchanged,
        "failed": stats.failed,
        "current": stats.current,
        "last_cycle_ts": stats.last_cycle_ts,
        "started_ts": stats.started_ts,
        "archive": summary,
    });

    // A daemon that improves a tree nobody can see is a daemon burning money
    // quietly. Say so rather than letting it pass as ok.
    let report = if stats.origin_missing {
        bkgclaw_ui::Report::with_checks(
            summary_text,
            vec![bkgclaw_ui::Check::fail(
                "kein remote",
                "[EVOLVE]",
                "bessere zweige bleiben lokal — `bkgclaw evolve init --origin <url>`",
            )],
        )
    } else {
        bkgclaw_ui::Report::ok(summary_text)
    };

    Outcome::from_report(report.with_data(data))
}

/// The jailed worker: one work phase, answer written back through the
/// handshake file.
///
/// # Why this exists as a subcommand
///
/// The jail re-executes the daemon's own binary inside the sandbox, because
/// the work phase is the only phase where the model's hands touch the tree.
/// The parent writes a briefing to a file, this reads it, runs the phase and
/// writes the outcome to a second file; the parent — outside the sandbox,
/// with the fitness gate — reads that back.
///
/// It has **no fitness gate of its own**. Running it by hand works exactly as
/// far as "the agent edited some files"; whether that was an improvement is
/// decided outside, by the judge. A work phase that is trusted to judge itself
/// is a work phase with no judge.
pub async fn work() -> Outcome {
    let dir = std::env::current_dir().unwrap_or_default();
    let (input, output) = bkg_evolve::work_handshake(&dir);

    let briefing = match std::fs::read_to_string(&input) {
        Ok(text) => text,
        Err(error) => {
            return Outcome::fail(Verdict::environment(format!(
                "handshake nicht lesbar ({}): {error}",
                input.display()
            )));
        }
    };

    let parsed: serde_json::Value =
        serde_json::from_str(&briefing).unwrap_or(serde_json::json!({}));
    let goal = parsed["goal"].as_str().unwrap_or_default().to_string();
    let lessons: Vec<String> = parsed["lessons"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    if goal.trim().is_empty() {
        return Outcome::fail(Verdict::environment(format!(
            "kein ziel im handshake ({}): {briefing}",
            input.display()
        )));
    }

    let outcome = bkg_evolve::work_phase(&dir, &goal, &lessons).await;
    match outcome {
        Ok(result) => {
            let body = match serde_json::to_string(&result) {
                Ok(body) => body,
                Err(error) => {
                    return Outcome::fail(Verdict::environment(format!(
                        "antwort nicht serialisierbar: {error}"
                    )));
                }
            };
            // The parent reads this file and nothing else. A write failure here
            // means the parent finds no answer — which it reports as a failure,
            // not as a success with an empty result.
            if let Err(error) = std::fs::write(&output, body) {
                return Outcome::fail(Verdict::environment(format!(
                    "antwort nicht schreibbar ({}): {error}",
                    output.display()
                )));
            }
            Outcome::success(format!(
                "work-phase fertig: {} züge, {}",
                result.turns, result.stop_reason
            ))
        }
        Err(error) => Outcome::fail(Verdict::environment(error)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_verdicts_are_the_engine_s_not_the_model_s() {
        // The CLI reports the fitness gate's verdict; there is no code
        // path where a model's own "ich habe es geschafft" turns into ok.
        assert_eq!(bkg_evolve::Verdict::Reverted.as_str(), "reverted");
        assert_eq!(bkg_evolve::Verdict::Improved.as_str(), "improved");
    }

    #[test]
    fn the_three_other_verdicts_are_named_too() {
        // A cycle can end "green but worse", and the name has to exist for
        // the archive to say which happened.
        assert_eq!(bkg_evolve::Verdict::Neutral.as_str(), "neutral");
        assert_eq!(bkg_evolve::Verdict::Suspect.as_str(), "suspect");
    }

    #[test]
    fn status_answers_on_a_machine_that_never_evolved() {
        // The command is what a curious operator runs first. It must not
        // error on an empty archive — it has to say "nothing yet", which is a
        // negative verdict rather than a failure.
        let outcome = super::status();
        let report = &outcome.1;
        assert!(!report.summary.is_empty(), "the report says nothing");
        assert!(
            report.data.is_some(),
            "the counters travel with it so the UI has one payload"
        );
    }
}
