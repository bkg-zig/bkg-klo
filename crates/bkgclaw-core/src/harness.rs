//! Harness CLIs as hireable providers (Task 02).
//!
//! A harness CLI is not an `OpenAiCompatible` model: `NIM_MODELS` stays the
//! live-verified model catalogue. A harness is a local binary that drives a
//! model. This module is the single catalogue both `bkgclaw-office` (who may
//! be hired) and the gateway/CLI (which binary to spawn) read, so the two can
//! never drift.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCli {
    pub name: &'static str,
    pub binary: &'static str,
    pub default_model: &'static str,
    pub sprite: &'static str,
}

/// Hire order is fallback order: when `claude-code` answers 404 / is missing,
/// the next entry is tried. `opencode` directly after `claude-code` is the
/// contract from the master prompt.
pub const HARNESS_CLIS: &[HarnessCli] = &[
    HarnessCli {
        name: "claude-code",
        binary: "claude",
        default_model: "claude-sonnet-4",
        sprite: "agent_typing_02.png",
    },
    HarnessCli {
        name: "opencode",
        binary: "opencode",
        default_model: "nvidia/nemotron-3-super-120b-a12b",
        sprite: "agent_walking_right_01.png",
    },
    HarnessCli {
        name: "codex",
        binary: "codex",
        default_model: "gpt-5",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "grok",
        binary: "grok",
        default_model: "grok-2-mini",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "kimi",
        binary: "kimi",
        default_model: "kimi-k2",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "qwen",
        binary: "qwen",
        default_model: "qwen2-0.5b-instruct",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "copilot",
        binary: "copilot",
        default_model: "claude-haiku-4-5",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "cursor",
        binary: "cursor",
        default_model: "claude-sonnet-4",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "antigravity",
        binary: "antigravity",
        default_model: "gemini-2.5-pro",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "crush",
        binary: "crush",
        default_model: "claude-sonnet-4",
        sprite: "agent_celebrating_01.png",
    },
    HarnessCli {
        name: "pi",
        binary: "pi",
        default_model: "llama3.2",
        sprite: "agent_celebrating_01.png",
    },
];

pub fn harness(name: &str) -> Option<&'static HarnessCli> {
    HARNESS_CLIS.iter().find(|h| h.name == name)
}

pub fn harness_names() -> Vec<&'static str> {
    HARNESS_CLIS.iter().map(|h| h.name).collect()
}

/// Next harness in failover order, `None` at the end of the chain.
pub fn next_harness(name: &str) -> Option<&'static HarnessCli> {
    let idx = HARNESS_CLIS.iter().position(|h| h.name == name)?;
    HARNESS_CLIS.get(idx + 1)
}

/// Command line for one task, e.g. `opencode run --model <m> <task>`.
pub fn args_for(h: &HarnessCli, task: &str) -> Vec<String> {
    match h.name {
        "opencode" => vec![
            "run".into(),
            "--model".into(),
            h.default_model.into(),
            task.into(),
        ],
        "claude-code" => vec![
            "--model".into(),
            h.default_model.into(),
            "--task".into(),
            task.into(),
        ],
        "codex" => vec!["--model".into(), h.default_model.into(), task.into()],
        _ => vec!["--model".into(), h.default_model.into(), task.into()],
    }
}

/// Whether `binary` is on `PATH` (pure PATH scan, no spawn).
pub fn binary_available(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
}

/// Resolve `requested`, falling forward while the binary is unavailable.
/// `available` is injectable so the chain is testable without a machine.
pub fn resolve_with(
    requested: &str,
    available: impl Fn(&str) -> bool,
) -> Result<&'static HarnessCli, String> {
    let start = harness(requested).ok_or_else(|| {
        format!(
            "unsupported harness cli {requested:?} (supported: {})",
            harness_names().join(", ")
        )
    })?;
    let mut current = Some(start);
    while let Some(h) = current {
        if available(h.binary) {
            return Ok(h);
        }
        current = next_harness(h.name);
    }
    Err(format!(
        "no harness binary available from {requested} onward"
    ))
}

pub fn resolve(requested: &str) -> Result<&'static HarnessCli, String> {
    resolve_with(requested, binary_available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_fails_over_to_opencode() {
        assert_eq!(
            next_harness("claude-code").map(|h| h.name),
            Some("opencode")
        );
        assert!(next_harness("pi").is_none());
        assert!(harness("emacs").is_none());
    }

    #[test]
    fn args_carry_the_default_model() {
        let opencode = harness("opencode").unwrap();
        let args = args_for(opencode, "BKG-001");
        assert!(args.iter().any(|a| a.contains("nemotron")), "{args:?}");
        assert!(args.iter().any(|a| a == "BKG-001"));
    }

    #[test]
    fn resolve_skips_missing_binaries() {
        // Only opencode installed: claude-code must land on opencode.
        let picked = resolve_with("claude-code", |b| b == "opencode").unwrap();
        assert_eq!(picked.name, "opencode");
        assert!(resolve_with("codex", |_| false).is_err());
    }
}
