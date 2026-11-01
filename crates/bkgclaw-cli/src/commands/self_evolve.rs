//! `bkgclaw self-evolve` (Task 04): read `.goals/` + MEMORY, then evolve.
//!
//! Without `--run` this is a read-only proposal listing. With `--run` the
//! first proposal goes through the fitness-gated `run_cycle` — the model
//! never commits, the gate does.

use crate::outcome::Outcome;
use bkgclaw_core::Verdict;

pub fn list() -> Outcome {
    let ev = bkg_evolve::EvolutionLoop::default();
    match ev.analyze() {
        Ok(props) if props.is_empty() => Outcome::from_report(bkgclaw_ui::Report::negative(
            "keine proposals",
            "ziele ablegen in .goals/*.md oder als '- evolve: ...' in .bkgclaw/MEMORY.md",
        )),
        Ok(props) => {
            let checks = props
                .iter()
                .map(|p| bkgclaw_ui::Check {
                    name: p.id.clone(),
                    ok: true,
                    code: "[GOAL]".into(),
                    detail: format!("{} ({})", p.goal, p.source),
                })
                .collect();
            Outcome::from_report(bkgclaw_ui::Report::with_checks(
                format!("{} proposals", props.len()),
                checks,
            ))
        }
        Err(e) => Outcome::fail(Verdict::environment(e)),
    }
}

pub async fn run_first(push: bool) -> Outcome {
    let ev = bkg_evolve::EvolutionLoop::default();
    let props = match ev.analyze() {
        Ok(p) => p,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    let Some(first) = props.into_iter().next() else {
        return Outcome::from_report(bkgclaw_ui::Report::negative(
            "keine proposals",
            "ziele ablegen in .goals/*.md",
        ));
    };
    let dir = std::env::current_dir().unwrap_or_default();
    match ev.apply(&dir, &first, push).await {
        Ok(result) => Outcome::success(format!(
            "self-evolve {} [{}] {}",
            result.id,
            result.verdict.as_str(),
            result.lesson
        )),
        Err(e) => Outcome::fail(Verdict::environment(e)),
    }
}
