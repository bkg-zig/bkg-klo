//! The cockpit views: one signal, four panels, zero hidden state.

use crate::net;
use crate::{ApprovalDecision, Cockpit, Panel, TranscriptItem, WireClient, WireEvent};
use dioxus::prelude::*;

#[component]
pub fn App() -> Element {
    let mut cockpit = use_signal(|| Cockpit {
        base: net::resolve_base(),
        policy: "allow-read-only".into(),
        ..Default::default()
    });

    // Boot: socket, sessions, a fresh session to talk to. One shot, not a
    // render effect — the cockpit owns exactly one of each.
    use_hook(|| {
        let base = cockpit.read().base.clone();
        let mut cockpit_for_socket = cockpit;
        spawn(async move {
            if net::ensure_socket(&base, cockpit_for_socket).is_none() {
                cockpit_for_socket.write().notice = "websocket nicht erreichbar".into();
            }
        });
    });

    let mut first_boot = use_signal(|| false);
    if !first_boot() {
        first_boot.set(true);
        let base = cockpit.read().base.clone();
        spawn(async move {
            // Fetch everything first, write once at the end: a signal's
            // write guard must never live across an await, or the UI
            // stops rendering mid-flight.
            let models = net::fetch_models(&base).await;
            let sessions = net::fetch_sessions(&base).await.unwrap_or_default();
            let session = net::create_session(&base, None, "allow-read-only".to_string()).await.ok();
            let mut state = cockpit.write();
            state.models = models;
            state.sessions = sessions;
            if let Some(session) = session {
                net::send(&WireClient::Subscribe { session: session.clone() });
                state.session = Some(session);
            } else {
                state.notice = "sitzung konnte nicht erstellt werden".into();
            }
        });
    }

    rsx! {
        // The stylesheet is bundled through manganis (`asset!`): dx gives it
        // a hashed name and this link is what actually loads it.
        document::Link { rel: "stylesheet", href: asset!("/assets/style.css") }

        div { class: "cockpit",
            header { class: "top",
                h1 { "bkgclaw" }
                select {
                    class: "model",
                    onchange: move |event| {
                        let model = event.value();
                        cockpit.write().notice = format!("modell {model} gilt ab nächster neuer sitzung");
                        cockpit.write().chosen_model = Some(model);
                    },
                    option { value: "", "failover-kette" }
                    for model in cockpit.read().models.clone().into_iter() {
                        option { value: "{model}" }
                    }
                }
                span { class: "notice", "{cockpit.read().notice}" }
                nav {
                    button {
                        class: if cockpit.read().panel == Panel::Chat { "on" },
                        onclick: move |_| cockpit.write().panel = Panel::Chat,
                        "chat"
                    }
                    button {
                        class: if cockpit.read().panel == Panel::Tasks { "on" },
                        onclick: move |_| {
                            cockpit.write().panel = Panel::Tasks;
                            let base = cockpit.read().base.clone();
                            spawn(async move {
                                let tasks = net::fetch_tasks(&base).await;
                                cockpit.write().tasks = tasks;
                            });
                        },
                        "aufgaben"
                    }
                    button {
                        class: if cockpit.read().panel == Panel::Memory { "on" },
                        onclick: move |_| {
                            cockpit.write().panel = Panel::Memory;
                            let base = cockpit.read().base.clone();
                            spawn(async move {
                                let memory = net::fetch_memory(&base).await;
                                cockpit.write().memory = memory;
                            });
                        },
                        "gedächtnis"
                    }
                    button {
                        class: if cockpit.read().panel == Panel::Skills { "on" },
                        onclick: move |_| {
                            cockpit.write().panel = Panel::Skills;
                            let base = cockpit.read().base.clone();
                            spawn(async move {
                                let skills = net::fetch_skills(&base).await;
                                cockpit.write().skills = skills;
                            });
                        },
                        "skills"
                    }
                    button {
                        class: if cockpit.read().panel == Panel::Evolve { "on" },
                        onclick: move |_| {
                            cockpit.write().panel = Panel::Evolve;
                            let base = cockpit.read().base.clone();
                            spawn(async move {
                                let evolve = net::fetch_evolve(&base).await;
                                cockpit.write().evolve = evolve;
                            });
                        },
                        "evolve"
                    }
                }
            }

            div { class: "body",
                aside { class: "side",
                    button {
                        class: "new",
                        onclick: move |_| {
                            let mut state = cockpit.write();
                            state.pending_new = true;
                            net::send(&WireClient::CreateSession {
                                model: state.chosen_model.clone(),
                                policy: Some(state.policy.clone()),
                                fork_from: None,
                                fork_at: None,
                            });
                        },
                        "+ neue sitzung"
                    }
                    for row in cockpit.read().sessions.clone().into_iter() {
                        button {
                            class: "session",
                            onclick: move |_| {
                                let session = row.id.clone();
                                let base = cockpit.read().base.clone();
                                spawn(async move {
                                    let transcript = net::fetch_transcript(&base, &session).await;
                                    let old = cockpit.read().session.clone();
                                    if let Some(old) = old {
                                        net::send(&WireClient::StopReceiving { session: old });
                                    }
                                    net::send(&WireClient::Subscribe { session: session.clone() });
                                    let mut state = cockpit.write();
                                    state.session = Some(session);
                                    state.transcript = transcript;
                                    state.live.clear();
                                    state.live_reasoning.clear();
                                    state.panel = Panel::Chat;
                                });
                            },
                            "{row.id}"
                            small { "{row.preview}" }
                        }
                    }
                }

                main { class: "main",
                    match cockpit.read().panel {
                        Panel::Chat => rsx! { Chat { cockpit } },
                        Panel::Tasks => rsx! { TaskPanel { cockpit } },
                        Panel::Memory => rsx! { MemoryPanel { cockpit } },
                        Panel::Skills => rsx! { SkillPanel { cockpit } },
                        Panel::Evolve => rsx! { EvolvePanel { cockpit } },
                    }
                }
            }
        }
    }
}

/// One pending approval, taken straight from the protocol's own event.
#[component]
fn ApprovalBar(mut cockpit: Signal<Cockpit>, pending: WireEvent) -> Element {
    let WireEvent::ApprovalRequest {
        call_id,
        name,
        risk,
        arguments,
        ..
    } = &pending
    else {
        return rsx! {};
    };
    // Each button needs its own id: a move-closure owns what it captures,
    // and three closures cannot share one.
    let allow_id = call_id.clone();
    let deny_id = call_id.clone();
    let always_id = call_id.clone();
    let tool = name.clone();
    let risk_text = risk.clone();
    let args = format!("{arguments}");
    rsx! {
        div { class: "approval",
            strong { "freigabe nötig: " }
            span { "{tool} ({risk_text})" }
            code { "{args}" }
            div { class: "choices",
                button {
                    onclick: move |_| {
                        net::send(&WireClient::Approval {
                            call_id: allow_id.clone(),
                            decision: ApprovalDecision::Allow,
                        });
                        cockpit.write().pending_approval = None;
                    },
                    "erlauben"
                }
                button {
                    onclick: move |_| {
                        net::send(&WireClient::Approval {
                            call_id: deny_id.clone(),
                            decision: ApprovalDecision::Deny,
                        });
                        cockpit.write().pending_approval = None;
                    },
                    "ablehnen"
                }
                button {
                    onclick: move |_| {
                        net::send(&WireClient::Approval {
                            call_id: always_id.clone(),
                            decision: ApprovalDecision::Always,
                        });
                        cockpit.write().pending_approval = None;
                    },
                    "immer erlauben"
                }
            }
        }
    }
}

/// A finished agent message, rendered as Markdown.
///
/// # Why `dangerous_inner_html` and not RSX elements
///
/// The alternative is walking the parsed tree and emitting one Dioxus element
/// per node. That is safer by construction — Dioxus escapes text nodes itself —
/// but it would mean a *third* renderer with its own test path, and the shared
/// crate's whole reason for existing is that both clients read one tree.
///
/// So the safety has to come from somewhere real, and it does:
/// `bkgclaw_markdown::backend::html` escapes every text node it emits and
/// writes only tags it chose itself. Raw HTML in model output is treated as
/// *text* by the parser and escaped again on the way out — two independent
/// passes. The unit tests in that module cover `<script>`, `<img onerror>`,
/// `javascript:` and `data:` URLs, which is where the risk actually lives.
///
/// `use_hypertext` keeps this syntactically visible: it is not a string that
/// might one day be built from unescaped fragments, it is a call that says
/// "this is trusted HTML", and the only producer is the escaping renderer.
#[component]
fn Markdown(source: String) -> Element {
    let html = use_memo(move || {
        let document = bkgclaw_markdown::parse(&source);
        bkgclaw_markdown::backend::html::render(&document)
    });
    rsx! {
        div {
            class: "msg agent md",
            dangerous_inner_html: "{html}",
        }
    }
}

/// Send the current input to the active session. Shared by the form and
/// the Enter key so both do exactly the same thing.
fn submit_message(cockpit: &mut Signal<Cockpit>) {
    let trimmed = cockpit.read().input.trim().to_string();
    if trimmed.is_empty() {
        return;
    }
    let session = cockpit.read().session.clone();
    let Some(session) = session else { return };
    cockpit.write().input.clear();
    cockpit.write().transcript.push(TranscriptItem::User(trimmed.clone()));
    net::send(&WireClient::Send { session, message: trimmed });
}

#[component]
fn Chat(mut cockpit: Signal<Cockpit>) -> Element {
    rsx! {
        div { class: "chat",
            if let Some(pending) = cockpit.read().pending_approval.clone() {
                ApprovalBar { cockpit, pending }
            }

            div { class: "transcript",
                for item in cockpit.read().transcript.clone().into_iter() {
                    match item {
                        TranscriptItem::User(text) => rsx! {
                            div { class: "msg user", "{text}" }
                        },
                        TranscriptItem::Assistant(text) => rsx! {
                            Markdown { source: text }
                        },
                        TranscriptItem::Reasoning(text) => rsx! {
                            div { class: "msg think", "{text}" }
                        },
                        TranscriptItem::Tool { name, outcome, output } => rsx! {
                            div { class: "msg tool {outcome}",
                                "⚙ {name}: "
                                code { "{output}" }
                            }
                        },
                        TranscriptItem::Error(text) => rsx! { div { class: "msg error", "{text}" } },
                        TranscriptItem::Status(text) => rsx! {
                            div { class: "msg notice", "· {text}" }
                        },
                    }
                }
                if !cockpit.read().live_reasoning.is_empty() {
                    // The streaming buffers stay plain text on purpose. Parsing
                    // a half-arrived message means re-parsing it on every delta,
                    // and the intermediate result flickers — a heading that has
                    // not received its `#` yet renders as a paragraph, then
                    // jumps. It becomes Markdown the moment the turn ends.
                    div { class: "msg think", "{cockpit.read().live_reasoning}" }
                }
                if !cockpit.read().live.is_empty() {
                    div { class: "msg agent live", "{cockpit.read().live}" }
                }
            }

            form {
                class: "input",
                // The native default would navigate away from the app;
                // the SPA stays.
                onsubmit: move |event| {
                    event.prevent_default();
                    submit_message(&mut cockpit);
                },
                textarea {
                    name: "message",
                    rows: "2",
                    placeholder: "nachricht an den agenten — enter sendet, shift+enter neue zeile",
                    value: "{cockpit.read().input}",
                    oninput: move |event| {
                        cockpit.write().input = event.value();
                    },
                    onkeydown: move |event| {
                        // Enter sends; Shift+Enter is the newline, because a
                        // textarea's native Enter would insert a line break
                        // and never submit the form.
                        if event.data().code() == dioxus::prelude::Code::Enter
                            && !event.data().modifiers().shift()
                        {
                            event.prevent_default();
                            submit_message(&mut cockpit);
                        }
                    }
                }
                div { class: "actions",
                    span { class: "cost", "${cockpit.read().spent:.4}" }
                    if cockpit.read().busy {
                        button {
                            class: "cancel",
                            onclick: move |_| {
                                let session = cockpit.read().session.clone().unwrap_or_default();
                                net::send(&WireClient::Cancel { session });
                            },
                            "turn abbrechen"
                        }
                    }
                    button { r#type: "submit", "senden" }
                }
            }
        }
    }
}

#[component]
fn TaskPanel(mut cockpit: Signal<Cockpit>) -> Element {
    rsx! {
        div { class: "panel",
            if cockpit.read().tasks.is_empty() {
                p { "keine aufgaben im workspace" }
            }
            for task in cockpit.read().tasks.clone().into_iter() {
                div { class: "row",
                    span { class: "status {task.status}", "[{task.status}]" }
                    strong { "{task.title}" }
                    small { "({task.slug})" }
                }
            }
            p { class: "hint", "aufgaben sind dateien in .bkgclaw/tasks — der agent pflegt sie mit task_add/task_update" }
        }
    }
}

#[component]
fn MemoryPanel(mut cockpit: Signal<Cockpit>) -> Element {
    rsx! {
        div { class: "panel",
            if cockpit.read().memory.is_empty() {
                p { "keine langzeit-einträge" }
            }
            for entry in cockpit.read().memory.clone().into_iter() {
                div { class: "row",
                    strong { "{entry.key}" }
                    span { "{entry.value}" }
                }
            }
            p { class: "hint", "heiße fakten: .bkgclaw/MEMORY.md — langzeit: memory_set / memory_search" }
        }
    }
}

#[component]
fn SkillPanel(mut cockpit: Signal<Cockpit>) -> Element {
    rsx! {
        div { class: "panel",
            if cockpit.read().skills.is_empty() {
                p { "keine skills installiert" }
            }
            for skill in cockpit.read().skills.clone().into_iter() {
                div { class: "row",
                    strong { "{skill.name}" }
                    span { "{skill.description}" }
                }
            }
            p { class: "hint", "skills liegen in .bkgclaw/skills/<name>/SKILL.md — der agent lädt sie mit skill_read" }
        }
    }
}

/// The level word of a log line, so it can be styled.
///
/// The daemon writes `<stamp> <level> <message>`; the level is the second
/// field. Scanning for a known word anywhere in the line is deliberate — the
/// level has to survive a message that happens to contain the word too.
fn level_of(line: &str) -> &'static str {
    for level in ["better", "fail", "park", "warn", "work", "start", "same"] {
        if line.contains(level) {
            return match level {
                "better" => "better",
                "fail" => "fail",
                "park" => "park",
                "warn" => "warn",
                "work" => "work",
                "start" => "start",
                _ => "same",
            };
        }
    }
    "same"
}

/// What the self-improvement daemon has done, is doing, and has queued.
///
/// # What this panel is for
///
/// A daemon that improves a tree nobody is watching looks exactly like a daemon
/// that is stuck. So the counters that tell the two apart are the headline,
/// not a footnote:
///
/// - `current` names the goal in flight — an empty one between cycles is
///   normal, a frozen one is not;
/// - `cycles` moves even when nothing improved, because a counter that only
///   ticks on success cannot distinguish "busy" from "hung";
/// - `origin_missing` is shouted about, because an improved branch nobody can
///   see is a budget spent for nothing.
#[component]
fn EvolvePanel(mut cockpit: Signal<Cockpit>) -> Element {
    let evolve = cockpit.read().evolve.clone();
    let daemon = &evolve.daemon;
    // RSX does not interpolate, so the labels are built before the markup.
    let queue_heading = format!("warteschlange ({})", evolve.queue.len());
    let attempts_heading = format!("versuche ({})", evolve.attempts.len());

    rsx! {
        div { class: "panel evolve",
            // The headline row: counters, not decoration.
            div { class: "evolve-stats",
                div { class: "stat big",
                    span { class: "stat-value", "{daemon.cycles}" }
                    span { class: "stat-label", "zyklen" }
                },
                div { class: "stat big good",
                    span { class: "stat-value", "{daemon.improved}" }
                    span { class: "stat-label", "verbessert" }
                },
                div { class: "stat",
                    span { class: "stat-value", "{daemon.unchanged}" }
                    span { class: "stat-label", "unverändert" }
                },
                div { class: if daemon.failed > 0 { "stat bad" } else { "stat" },
                    span { class: "stat-value", "{daemon.failed}" }
                    span { class: "stat-label", "fehlgeschlagen" }
                },
            }

            // What it is doing right now. Empty between cycles is normal, and
            // saying so beats a blinking cursor that means nothing.
            div { class: if daemon.current.is_some() { "evolve-now busy" } else { "evolve-now" },
                if let Some(goal) = &daemon.current {
                    span { class: "pulse", "●" }
                    " arbeitet an: "
                    strong { "{goal}" }
                } else {
                    "wartet auf arbeit"
                }
            }

            if daemon.origin_missing {
                div { class: "warn-banner",
                    "kein push-remote — bessere zweige bleiben lokal. ",
                    code { "bkgclaw evolve init --origin <url>" }
                }
            }

            h3 { "{queue_heading}" }
            if evolve.queue.is_empty() {
                p { class: "hint",
                    "keine ziele in der warteschlange. ",
                    code { "bkgclaw evolve seed \"verbessere die tui\"" }
                }
            } else {
                for goal in evolve.queue.iter() {
                    div { class: if goal.parked { "row parked" } else { "row" },
                        span { class: if goal.parked { "status parked" } else { "status" },
                            if goal.parked { " geparkt" } else { " wartend" }
                        }
                        strong { "{goal.goal}" }
                        small { "versuche {goal.attempts}" }
                    }
                }
            }

            h3 { "{attempts_heading}" }
            if evolve.attempts.is_empty() {
                p { class: "hint", "noch keine versuche — der daemon hat noch nichts getan" }
            } else {
                for attempt in evolve.attempts.iter().take(30) {
                    div { class: "row",
                        span { class: "status {attempt.verdict}", "[{attempt.verdict}]" }
                        strong { "#{attempt.id} {attempt.goal}" }
                        small { "{attempt.tests_passed}/{attempt.tests_failed} tests, {attempt.clippy_warnings} clippy" }
                        if !attempt.lesson.is_empty() {
                            p { class: "lesson", "{attempt.lesson}" }
                        }
                    }
                }
            }

            // The live channel. Appended to by the daemon as each event happens, so what
            // is here is what it is doing *now* — not a snapshot from the last
            // click. Newest last, like `tail -f`.
            h3 { "live-log" }
            if evolve.changelog.is_empty() {
                p { class: "hint", "noch nichts protokolliert — ",
                    code { "tail -f {evolve.changelog_path}" }
                }
            } else {
                pre { class: "changelog",
                    for line in evolve.changelog.iter().rev().take(60) {
                        div { class: "logline {level_of(line)}", "{line}" }
                    }
                }
            }

            if !evolve.journal_head.is_empty() {
                h3 { "journal" }
                pre { class: "journal", "{evolve.journal_head}" }
            }

            p { class: "hint",
                "queue: ", code { "{evolve.queue_path}" }
            }
        }
    }
}
