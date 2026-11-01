//! WebLLM ORC (Task 03): the browser-side orchestrator contract.
//!
//! The model itself (`@mlc-ai/web-llm`, Qwen2-0.5B) runs in the browser; this
//! module is the Rust mirror of its contract so the gateway, the CLI and the
//! static pages agree on model, fallbacks, system prompt and tool routing.
//! Nothing here loads WebGPU — it decides *what* the page may ask for.

use serde::{Deserialize, Serialize};

/// Primary browser model: 500 MB, WebGPU-fast. Fallbacks in load order.
pub const WEBLLM_MODEL: &str = "Qwen2-0.5B-Instruct-q4f16_1-MLC";
pub const WEBLLM_FALLBACKS: &[&str] = &[
    "Phi-3-mini-q4f16_1-MLC",
    "Llama-3.2-1B-Instruct-q4f16_1-MLC",
];

pub const ORC_SYSTEM_PROMPT: &str = "You are Michael, GOD orchestrator, route via tools, like gas station. \
Stations: CODE PUMP (coding) -> TEST SHOP (testing) -> REVIEW COUNTER (review) -> DEPLOY PUMP (deploy) -> BREAK. \
A task may only start when its dependency is done, like tanken then bezahlen. \
Answer with one tool call in JSON: {\"tool\": \"<name>\", \"args\": {...}}.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebLlmConfig {
    pub model: String,
    pub fallbacks: Vec<String>,
    pub system_prompt: String,
}

impl Default for WebLlmConfig {
    fn default() -> Self {
        Self {
            model: WEBLLM_MODEL.into(),
            fallbacks: WEBLLM_FALLBACKS.iter().map(|s| s.to_string()).collect(),
            system_prompt: ORC_SYSTEM_PROMPT.into(),
        }
    }
}

/// The nine WebMCP tools the floor exposes. Risk follows `tools::Risk`:
/// reads are read-only, floor writes mutating, spawn/approve need the gate.
pub const WEBMCP_TOOLS: &[(&str, &str)] = &[
    ("place_desk", "mutating"),
    ("place_chair", "mutating"),
    ("place_agent", "mutating"),
    ("move_agent", "mutating"),
    ("send_envelope", "read-only"),
    ("assign_task", "mutating"),
    ("set_agent_status", "mutating"),
    ("spawn_agent", "destructive"),
    ("approve", "destructive"),
];

pub fn webmcp_tool_names() -> Vec<&'static str> {
    WEBMCP_TOOLS.iter().map(|(n, _)| *n).collect()
}

/// Route one orchestrator sentence to a tool name. Deterministic on purpose:
/// the browser model proposes, this table disposes — an unknown intent maps
/// to `send_envelope` (harmless) instead of guessing a destructive tool.
pub fn route_intent(text: &str) -> &'static str {
    let t = text.to_lowercase();
    if t.contains("spawn") || t.contains("hire") || t.contains("einstellen") {
        "spawn_agent"
    } else if t.contains("desk") || t.contains("schreibtisch") {
        "place_desk"
    } else if t.contains("chair") || t.contains("stuhl") {
        "place_chair"
    } else if t.contains("move") || t.contains("beweg") || t.contains("lauf") {
        "move_agent"
    } else if t.contains("task") || t.contains("aufgabe") || t.contains("assign") {
        "assign_task"
    } else if t.contains("status") {
        "set_agent_status"
    } else if t.contains("approve") || t.contains("freigabe") || t.contains("genehmig") {
        "approve"
    } else if t.contains("agent") && (t.contains("place") || t.contains("stell")) {
        "place_agent"
    } else {
        "send_envelope"
    }
}

/// Parse the model's JSON tool call. Malformed JSON is an error, never a
/// silent default to a destructive tool.
pub fn parse_tool_call(raw: &str) -> Result<(String, serde_json::Value), String> {
    let value: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("tool call is not JSON: {e}"))?;
    let tool = value
        .get("tool")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "tool call has no \"tool\" field".to_string())?;
    if !webmcp_tool_names().contains(&tool) {
        return Err(format!("unknown webmcp tool {tool:?}"));
    }
    Ok((
        tool.to_string(),
        value.get("args").cloned().unwrap_or(serde_json::json!({})),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_intent_routes_to_spawn_agent() {
        assert_eq!(route_intent("Spawn 3 agents"), "spawn_agent");
        assert_eq!(route_intent("chatte nur"), "send_envelope");
        assert_eq!(WEBLLM_MODEL, "Qwen2-0.5B-Instruct-q4f16_1-MLC");
    }

    #[test]
    fn tool_calls_must_be_known_json() {
        let (tool, _) = parse_tool_call(r#"{"tool":"place_desk","args":{"x":3,"y":4}}"#).unwrap();
        assert_eq!(tool, "place_desk");
        assert!(parse_tool_call(r#"{"tool":"rm_rf"}"#).is_err());
        assert!(parse_tool_call("not json").is_err());
    }
}
