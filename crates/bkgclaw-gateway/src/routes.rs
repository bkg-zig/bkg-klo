//! REST handlers. Every endpoint answers JSON; the wire contract is
//! `{"ok": bool, "data": …}` so a client needs one parser, not one per
//! route.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use bkgclaw_core::tools::{Decision, Policy};
use bkgclaw_store::Session;

use crate::engine::start_turn;
use crate::events::{ApprovalDecision, Event};
use crate::state::Gateway;

/// `GET /api/health` — no auth: the answer contains nothing worth guarding.
pub async fn health(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let sessions = bkgclaw_store::Session::list(&gateway.home).len();
    Json(json!({
        "ok": true,
        "service": "bkgclaw-gateway",
        "nim_configured": gateway.has_nim(),
        "models": gateway.usable_models().len(),
        "sessions": sessions,
    }))
}

pub async fn models(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let models: Vec<serde_json::Value> = gateway
        .usable_models()
        .into_iter()
        .map(
            |(model, vendor, usable)| json!({ "model": model, "vendor": vendor, "usable": usable }),
        )
        .collect();
    Json(json!({ "ok": true, "data": models }))
}

pub async fn tools(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let registry = bkgclaw_core::tools::builtin_tools();
    let policy = *gateway.inner.default_policy.lock().expect("policy");
    let overrides = Gateway::interactive_overrides(policy);
    let data: Vec<serde_json::Value> = registry
        .all()
        .iter()
        .map(|tool| {
            // In the gateway, the interactive mapping decides: what would
            // silently run, and what surfaces as a question.
            let decision = registry
                .gate(&tool.name, Policy::AllowAll, &overrides)
                .decision;
            json!({
                "name": tool.name,
                "risk": tool.risk.as_str(),
                "interactive": match decision {
                    Decision::Allow => "auto",
                    Decision::Ask => "ask",
                    Decision::Deny => "deny",
                },
            })
        })
        .collect();
    Json(json!({ "ok": true, "data": data }))
}

#[derive(Deserialize)]
pub struct PolicyBody {
    pub policy: String,
}

pub async fn set_policy(
    State(gateway): State<Arc<Gateway>>,
    Json(body): Json<PolicyBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let policy = Policy::parse(&body.policy);
    // Round-trip through as_str: an unknown policy name must be visible in
    // the echo, not silently coerced.
    *gateway.inner.default_policy.lock().expect("policy") = policy;
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "data": { "policy": policy.as_str() } })),
    )
}

pub async fn sessions(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let data: Vec<serde_json::Value> = Session::list(&gateway.home)
        .into_iter()
        .map(|summary| serde_json::to_value(&summary).expect("summary serialises"))
        .collect();
    Json(json!({ "ok": true, "data": data }))
}

#[derive(Deserialize)]
pub struct CreateSession {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub fork_from: Option<String>,
    #[serde(default)]
    pub fork_at: Option<usize>,
}

pub async fn create_session(
    State(gateway): State<Arc<Gateway>>,
    Json(body): Json<CreateSession>,
) -> (StatusCode, Json<serde_json::Value>) {
    let policy = body
        .policy
        .map(|p| Policy::parse(&p).as_str().to_string())
        .unwrap_or_else(|| {
            gateway
                .inner
                .default_policy
                .lock()
                .expect("policy")
                .as_str()
                .to_string()
        });

    let session = match &body.fork_from {
        Some(source) => {
            let Some(slot) = gateway.slot(source) else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "ok": false, "error": format!("no session `{source}` to fork") })),
                );
            };
            let origin = slot.session.lock().expect("session").clone();
            origin.fork(body.fork_at.unwrap_or(usize::MAX))
        }
        None => Session::new(body.model, policy),
    };
    let id = session.id.clone();
    if session.save(&gateway.home).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": "could not persist the session" })),
        );
    }
    gateway.insert_session(session);
    gateway.publish(Event::SessionUpdated {
        session: id.clone(),
    });
    (
        StatusCode::CREATED,
        Json(json!({ "ok": true, "data": { "session": id } })),
    )
}

pub async fn get_session(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match gateway.slot(&id) {
        Some(slot) => {
            let session = slot.session.lock().expect("session").clone();
            let data = serde_json::to_value(&session).expect("session serialises");
            (StatusCode::OK, Json(json!({ "ok": true, "data": data })))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": format!("no session `{id}`") })),
        ),
    }
}

pub async fn delete_session(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    // A running session is not deletable: its task would commit into a
    // deleted file, which is a lie about what happened.
    if let Some(slot) = gateway.slot(&id) {
        if slot.busy.load(std::sync::atomic::Ordering::SeqCst) {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "ok": false, "error": "the session is running a turn" })),
            );
        }
    }
    if Session::delete(&gateway.home, &id) {
        gateway.inner.sessions.lock().expect("sessions").remove(&id);
        gateway.inner.logs.lock().expect("logs").remove(&id);
        (
            StatusCode::OK,
            Json(json!({ "ok": true, "data": { "deleted": id } })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": format!("no session `{id}`") })),
        )
    }
}

#[derive(Deserialize)]
pub struct SendMessage {
    pub message: String,
}

pub async fn send_message(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
    Json(body): Json<SendMessage>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Some(slot) = gateway.slot(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": format!("no session `{id}`") })),
        );
    };
    if body.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "an empty message runs nothing" })),
        );
    }
    match start_turn(&gateway, &slot, &body.message, None) {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(json!({ "ok": true, "data": { "session": id, "accepted": true } })),
        ),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({ "ok": false, "error": error })),
        ),
    }
}

#[derive(Deserialize)]
pub struct EventsQuery {
    since: Option<usize>,
}

pub async fn session_events(
    State(gateway): State<Arc<Gateway>>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
) -> Json<serde_json::Value> {
    let events = gateway.events_since(&id, query.since.unwrap_or(0));
    let data: Vec<serde_json::Value> = events
        .iter()
        .map(|event| serde_json::to_value(event).expect("event serialises"))
        .collect();
    Json(json!({
        "ok": true,
        "data": data,
        "next_since": query.since.unwrap_or(0) + data.len(),
    }))
}

#[derive(Deserialize)]
pub struct ApprovalBody {
    pub decision: ApprovalDecision,
}

/// `POST /api/approvals/{call_id}` — the REST path to answer an approval;
/// the WebSocket has its own. "always" grants the tool for the session.
pub async fn answer_approval(
    State(gateway): State<Arc<Gateway>>,
    Path(call_id): Path<String>,
    Json(body): Json<ApprovalBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Consume: one answer per approval; a second reply finds nothing.
    let sender = gateway
        .inner
        .approvals
        .lock()
        .expect("approvals")
        .remove(&call_id);
    let Some(sender) = sender else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "no approval is waiting for that call" })),
        );
    };

    let decision = match body.decision {
        ApprovalDecision::Allow | ApprovalDecision::Always => Decision::Allow,
        ApprovalDecision::Deny => Decision::Deny,
    };
    if body.decision == ApprovalDecision::Always {
        if let Some((session, tool)) = gateway
            .inner
            .approval_context
            .lock()
            .expect("approvals")
            .get(&call_id)
            .cloned()
        {
            if let Some(slot) = gateway.slot(&session) {
                slot.allowed.lock().expect("allowed").insert(tool.clone());
                gateway.publish(Event::SessionUpdated { session });
            }
        }
    }
    let delivered = sender.send(decision).is_ok();
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "data": { "delivered": delivered } })),
    )
}
pub async fn memory(State(gateway): State<Arc<Gateway>>) -> (StatusCode, Json<serde_json::Value>) {
    match bkgclaw_store::MemoryStore::load(&gateway.home) {
        Ok(store) => {
            let data: Vec<serde_json::Value> = store
                .entries()
                .iter()
                .map(|entry| serde_json::to_value(entry).expect("entry serialises"))
                .collect();
            (StatusCode::OK, Json(json!({ "ok": true, "data": data })))
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": error })),
        ),
    }
}

pub async fn skills(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let index = bkgclaw_store::SkillIndex::load(&gateway.workspace, &gateway.home);
    let data: Vec<serde_json::Value> = index
        .skills
        .iter()
        .map(|skill| {
            json!({ "name": skill.name, "description": skill.description, "source": skill.source })
        })
        .collect();
    Json(json!({ "ok": true, "data": data }))
}

/// `GET /api/evolve` — the attempt archive and journal head for the UIs.
pub async fn evolve_status(State(_gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let home = bkgclaw_store::home_root();
    let archive = bkg_evolve::AttemptArchive::new(&home);
    let attempts: Vec<serde_json::Value> = archive
        .all()
        .iter()
        .map(|attempt| {
            serde_json::json!({
                "id": attempt.id,
                "ts": attempt.ts,
                "goal": attempt.goal,
                "branch": attempt.branch,
                "verdict": attempt.verdict.as_str(),
                "tests_passed": attempt.fitness.tests_passed,
                "tests_failed": attempt.fitness.tests_failed,
                "clippy_warnings": attempt.fitness.clippy_warnings,
                "lesson": attempt.lesson,
            })
        })
        .collect();
    let journal = std::fs::read_to_string(bkg_evolve::journal_path()).unwrap_or_default();
    let head: String = journal.lines().take(80).collect::<Vec<_>>().join("\n");

    // The daemon's own view: counters and the queue it is working through.
    // Without these the UI could show history but not whether anything is
    // happening *now* — which is the question the panel exists to answer.
    let (stats, summary) = bkg_evolve::stats_for(&home);
    let queue = bkg_evolve::Queue::new(&home);
    let goals: Vec<serde_json::Value> = queue
        .goals()
        .into_iter()
        .map(|goal| {
            serde_json::json!({
                "goal": goal.goal,
                "attempts": goal.attempts,
                "push": goal.push,
                "parked": goal.attempts >= bkg_evolve::MAX_GOAL_ATTEMPTS,
            })
        })
        .collect();

    Json(json!({
        "ok": true,
        "data": {
            "attempts": attempts,
            "journal_head": head,
            "daemon": {
                "cycles": stats.cycles,
                "improved": stats.improved,
                "unchanged": stats.unchanged,
                "failed": stats.failed,
                "current": stats.current,
                "last_cycle_ts": stats.last_cycle_ts,
                "started_ts": stats.started_ts,
                "origin_missing": stats.origin_missing,
            },
            "queue": goals,
            "queue_path": bkg_evolve::queue_path(&home).display().to_string(),
            "archive": summary,
        }
    }))
}

#[derive(Deserialize)]
pub struct EvolveRunBody {
    pub goal: String,
    /// Working directory of the tree to evolve — must exist and be a cargo
    /// workspace; the daemon refuses to run evolution somewhere else.
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub push: bool,
}

/// `POST /api/evolve` — one evolution cycle, in the background. Progress
/// reaches every client through the event bus as notices; the result lands
/// in the attempt archive this endpoint reads.
pub async fn evolve_run(
    State(gateway): State<Arc<Gateway>>,
    Json(body): Json<EvolveRunBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let dir = body
        .dir
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !dir.join("Cargo.toml").is_file() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "ok": false, "error": format!("`{}` ist kein cargo-workspace", dir.display()) }),
            ),
        );
    }
    if body.goal.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "ein evolve-lauf braucht ein ziel" })),
        );
    }

    let goal = body.goal.clone();
    let push = body.push;
    let dir_display = dir.display().to_string();
    let dir_for_task = dir_display.clone();
    let event_gateway = gateway.clone();
    tokio::spawn(async move {
        let options = bkg_evolve::EvolveOptions {
            push,
            ..Default::default()
        };
        for notice in [format!("evolve startet: {goal}")] {
            event_gateway.publish(Event::Notice {
                session: "-".into(),
                text: notice,
            });
        }
        let dir = std::path::PathBuf::from(&dir_for_task);
        match bkg_evolve::run_cycle(&dir, &goal, &options).await {
            Ok(result) => {
                event_gateway.publish(Event::Notice {
                    session: "-".into(),
                    text: format!(
                        "evolve {id:03} [{verdict}] {lesson}{pushed}",
                        id = result.id,
                        verdict = result.verdict.as_str(),
                        lesson = result.lesson,
                        pushed = if result.pushed { " — gepusht" } else { "" },
                    ),
                });
            }
            Err(error) => {
                event_gateway.publish(Event::Notice {
                    session: "-".into(),
                    text: format!("evolve fehlgeschlagen: {error}"),
                });
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(json!({ "ok": true, "data": { "accepted": true, "dir": dir_display } })),
    )
}

fn office_load(workspace: &std::path::Path) -> Result<bkgclaw_office::OfficeHarness, String> {
    let path = workspace.join("office.json");
    if !path.exists() {
        return Ok(bkgclaw_office::OfficeHarness::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let office: bkgclaw_office::Office = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(bkgclaw_office::OfficeHarness { office })
}

fn office_save(
    workspace: &std::path::Path,
    h: &bkgclaw_office::OfficeHarness,
) -> Result<(), String> {
    std::fs::create_dir_all(workspace).map_err(|e| e.to_string())?;
    std::fs::write(
        workspace.join("office.json"),
        h.export_level().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Execute one WebMCP tool against the floor. Destructive tools
/// (`spawn_agent`, `approve`) need `approved:true` — the gate, not the model,
/// decides. Returns the tool result as JSON.
pub fn apply_office_tool(
    workspace: &std::path::Path,
    tool: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use bkgclaw_office::{EmployeeStatus, FloorTask, StationType, TaskStatus};
    let risk = bkgclaw_core::orc_webllm::WEBMCP_TOOLS
        .iter()
        .find(|(n, _)| *n == tool)
        .map(|(_, r)| *r);
    let Some(risk) = risk else {
        return Err(format!("unknown webmcp tool {tool:?}"));
    };
    if risk == "destructive" && args.get("approved").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!(
            "tool {tool} is destructive and needs approved:true (gate)"
        ));
    }
    let mut h = office_load(workspace)?;
    if h.office.desks.is_empty() && tool != "place_desk" {
        let (w, hh) = (h.office.width.max(20), h.office.height.max(14));
        h.setup_office_sync(w, hh).map_err(|e| e.to_string())?;
    }
    let num = |k: &str| args.get(k).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let text = |k: &str, d: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or(d)
            .to_string()
    };
    let result: serde_json::Value = match tool {
        "place_desk" | "place_chair" => {
            let asset = if tool == "place_chair" {
                text("asset", "chair_blue_01.png")
            } else {
                text("asset", "desk_dual_monitor_01.png")
            };
            let d = h
                .place_desk(num("x"), num("y"), &asset, StationType::CodePump)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(d).map_err(|e| e.to_string())?
        }
        "place_agent" | "spawn_agent" => {
            let emp = h
                .hire_employee_sync(
                    &text("cli", "opencode"),
                    &text("name", "Agent"),
                    &text("desk", "desk_snes_01.png"),
                )
                .map_err(|e| e.to_string())?;
            serde_json::to_value(emp).map_err(|e| e.to_string())?
        }
        "move_agent" => {
            let e = h
                .move_agent(&text("id", "emp_1"), num("x"), num("y"))
                .map_err(|e| e.to_string())?;
            serde_json::to_value(e).map_err(|e| e.to_string())?
        }
        "set_agent_status" => {
            let status = match text("status", "BlueReady").as_str() {
                "GreenActive" => EmployeeStatus::GreenActive,
                "AmberAttention" => EmployeeStatus::AmberAttention,
                "RedError" => EmployeeStatus::RedError,
                _ => EmployeeStatus::BlueReady,
            };
            let e = h
                .set_agent_status(&text("id", "emp_1"), status)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(e).map_err(|e| e.to_string())?
        }
        "send_envelope" => {
            let env = h.send_envelope(
                &text("from", "emp_1"),
                &text("to", "desk"),
                &text("text", "handoff"),
            );
            serde_json::to_value(env).map_err(|e| e.to_string())?
        }
        "assign_task" => {
            let task = FloorTask {
                id: text("id", "BKG-001"),
                title: text("title", "task"),
                station: StationType::CodePump,
                depends_on: args
                    .get("depends_on")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                status: TaskStatus::Pending,
                assignee: args
                    .get("assignee")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            };
            let t = h.assign_task(task).map_err(|e| e.to_string())?;
            serde_json::to_value(t).map_err(|e| e.to_string())?
        }
        "approve" => json!({ "approved": true, "call_id": text("call_id", "") }),
        _ => return Err(format!("unhandled tool {tool}")),
    };
    office_save(workspace, &h)?;
    Ok(result)
}

pub async fn office(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    match office_load(&gateway.workspace) {
        Ok(h) => Json(
            json!({ "ok": true, "data": serde_json::from_str::<serde_json::Value>(&h.export_level().unwrap_or_else(|_| "{}".into())).unwrap_or(json!({})) }),
        ),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

pub async fn office_agents(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    match office_load(&gateway.workspace) {
        Ok(h) => Json(json!({ "ok": true, "data": h.office.employees })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct OfficeToolBody {
    #[serde(default)]
    pub args: serde_json::Value,
}

pub async fn office_tool(
    State(gateway): State<Arc<Gateway>>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let args = body.get("args").cloned().unwrap_or(body);
    gateway.publish(Event::OfficeToolCall {
        session: "office".into(),
        tool: name.clone(),
        args: args.clone(),
    });
    match apply_office_tool(&gateway.workspace, &name, &args) {
        Ok(data) => {
            gateway.publish(Event::OfficeUpdated {
                session: "office".into(),
            });
            (StatusCode::OK, Json(json!({ "ok": true, "data": data })))
        }
        Err(e) if e.contains("needs approved") => (
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": e })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        ),
    }
}

#[cfg(test)]
mod office_tool_tests {
    use super::*;

    #[test]
    fn destructive_tools_need_the_gate_and_place_desk_persists() {
        let dir = std::env::temp_dir().join(format!("bkg-gw-office-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // spawn without approval is refused by the gate.
        assert!(apply_office_tool(&dir, "spawn_agent", &json!({"cli":"opencode"})).is_err());
        let placed = apply_office_tool(&dir, "place_desk", &json!({"x":3,"y":4})).unwrap();
        assert_eq!(placed["x"], 3);
        let hired = apply_office_tool(
            &dir,
            "spawn_agent",
            &json!({"cli":"opencode","name":"ViaAPI","approved":true}),
        )
        .unwrap();
        assert_eq!(hired["name"], "ViaAPI");
        assert!(dir.join("office.json").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub async fn tasks(State(gateway): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    let store = bkgclaw_store::TaskStore::new(&gateway.workspace);
    let data: Vec<serde_json::Value> = store
        .list()
        .iter()
        .map(|task| {
            json!({
                "slug": task.slug,
                "title": task.title,
                "status": task.status.as_str(),
                "updated_at": task.updated_at,
            })
        })
        .collect();
    Json(json!({ "ok": true, "data": data }))
}
