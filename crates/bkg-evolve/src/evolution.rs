//! Self-evolution loop (Task 04): goals on disk -> proposals -> fitness gate.
//!
//! `analyze` only reads (`.goals/*.md`, workspace MEMORY.md). `apply` runs one
//! real `run_cycle`: NIM generates the diff inside the agent turn,
//! `nemotron-3-nano-omni` is in the tool chain that judges, and the fitness
//! gate (cargo build/test/clippy exit codes) is the only committer. A passed
//! proposal is archived as `.bkgclaw/tasks/evolution-<ts>.md` by the caller.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::{EvolveOptions, run_cycle};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Proposal {
    pub id: String,
    pub goal: String,
    pub source: String,
}

pub struct EvolutionLoop {
    pub goals_dir: PathBuf,
    pub memory_path: PathBuf,
}

impl Default for EvolutionLoop {
    fn default() -> Self {
        Self {
            goals_dir: PathBuf::from(".goals"),
            memory_path: PathBuf::from(".bkgclaw/MEMORY.md"),
        }
    }
}

impl EvolutionLoop {
    pub fn new(goals_dir: impl Into<PathBuf>, memory_path: impl Into<PathBuf>) -> Self {
        Self {
            goals_dir: goals_dir.into(),
            memory_path: memory_path.into(),
        }
    }

    /// Read goals + hot memory into proposals. Missing dirs are empty, not errors.
    pub fn analyze(&self) -> Result<Vec<Proposal>, String> {
        let mut out = Vec::new();
        if self.goals_dir.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(&self.goals_dir)
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|x| x == "md").unwrap_or(false))
                .collect();
            files.sort();
            for path in files {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let goal = text
                    .lines()
                    .map(|l| l.trim_start_matches('#').trim())
                    .find(|l| !l.is_empty())
                    .unwrap_or("goal")
                    .to_string();
                out.push(Proposal {
                    id: path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    goal,
                    source: path.display().to_string(),
                });
            }
        }
        if let Ok(memory) = std::fs::read_to_string(&self.memory_path) {
            for line in memory.lines().filter(|l| l.starts_with("- evolve:")) {
                out.push(Proposal {
                    id: format!("memory-{}", out.len() + 1),
                    goal: line.trim_start_matches("- evolve:").trim().to_string(),
                    source: self.memory_path.display().to_string(),
                });
            }
        }
        Ok(out)
    }

    /// Run one proposal through the fitness-gated cycle in `dir`.
    pub async fn apply(
        &self,
        dir: &Path,
        proposal: &Proposal,
        push: bool,
    ) -> Result<crate::engine::CycleResult, String> {
        run_cycle(
            dir,
            &proposal.goal,
            &EvolveOptions {
                push,
                ..Default::default()
            },
        )
        .await
    }

    /// Archive path for a passed proposal: `.bkgclaw/tasks/evolution-<ts>.md`.
    pub fn task_path(workspace: &Path, ts: u64) -> PathBuf {
        workspace.join("tasks").join(format!("evolution-{ts}.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_reads_goal_headings() {
        let dir = std::env::temp_dir().join(format!("bkg-evo-{}", std::process::id()));
        let goals = dir.join(".goals");
        std::fs::create_dir_all(&goals).unwrap();
        std::fs::write(
            goals.join("goal_01_test.md"),
            "# Office ohne Mensch einrichten\n\nDetails.\n",
        )
        .unwrap();
        let ev = EvolutionLoop::new(&goals, dir.join("MEMORY.md"));
        let props = ev.analyze().unwrap();
        assert_eq!(props.len(), 1);
        assert!(props[0].goal.contains("Office ohne Mensch"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
