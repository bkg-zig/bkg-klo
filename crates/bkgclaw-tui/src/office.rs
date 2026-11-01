//! The office panel: gateway events become a picture of who is doing what.
//!
//! # What this adds to the TUI
//!
//! The transcript answers "what did the agent say". It cannot answer "what is
//! the agent doing *right now*", which is the question an operator with three
//! sessions running actually has. This module answers it visually: one figure
//! per session, typing while a turn runs, a `?` when it needs a human, dozing
//! when it has been quiet.
//!
//! # Why it is a separate module and not a widget in `ui.rs`
//!
//! `ui.rs` is pure rendering: state in, characters out. The office is the
//! same, but it needs a *second* piece of state — the room — which has a
//! lifecycle of its own (agents arrive and leave, poses decay over time, a
//! daemon appears and vanishes). Folding that into [`crate::state::TuiState`]
//! would mean every event that touches the transcript also had to reason about
//! floor plans.
//!
//! So: [`OfficePanel`] owns the room, [`feed`] folds one event into it, and
//! [`draw`] puts it on screen. Nothing else in the TUI needs to know a room
//! exists.

use bkgclaw_office::room::{Activity, Agent, Office};
use bkgclaw_office::scene;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::state::{Line, TuiState};

/// The office strip above the transcript.
#[derive(Debug)]
pub struct OfficePanel {
    room: Office,
    /// Milliseconds since the epoch, supplied by the caller. Kept out of the
    /// panel so a test can advance time without sleeping.
    now_ms: u64,
    /// Animation counter. Advances once per drawn frame, and only the typing
    /// pose reads it.
    beat: u64,
    /// Width the room was laid out for. A narrower terminal needs fewer desks,
    /// so a resize re-plans the floor.
    width: u16,
    height: u16,
}

/// Rows the office strip occupies when it is on.
pub const OFFICE_HEIGHT: u16 = 6;

impl OfficePanel {
    pub fn new(width: u16, height: u16) -> Self {
        // Braille packs four pixel rows into one cell, so the pixel floor is
        // four times taller than the strip.
        let room = Office::with_capacity(
            6,
            width * bkgclaw_office::braille::Render::Braille.sub_width(),
            height * bkgclaw_office::braille::Render::Braille.sub_height(),
        );
        OfficePanel {
            room,
            now_ms: 0,
            beat: 0,
            width,
            height,
        }
    }

    pub fn room(&self) -> &Office {
        &self.room
    }

    /// Advance the clock. Called once per frame from the event loop, so poses
    /// decay without any event having to say "still idle".
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        self.beat = self.beat.wrapping_add(1);
        // Sweep daemons on the same tick. A daemon that stopped talking is gone;
        // leaving it on the floor would be a lie about the machine's state.
        self.room.sweep_daemons(now_ms);
    }

    /// Re-plan the floor after a resize.
    ///
    /// The room is rebuilt rather than resized: a floor plan is a function of
    /// its size, and the alternative — moving desks around and hoping every
    /// agent still has one — is how two agents end up sharing a desk.
    pub fn resize(&mut self, width: u16, height: u16) {
        if width == self.width && height == self.height {
            return;
        }
        let mut rebuilt = Office::with_capacity(
            6,
            width * bkgclaw_office::braille::Render::Braille.sub_width(),
            height * bkgclaw_office::braille::Render::Braille.sub_height(),
        );
        // Keep whoever was already here, in the same order, so a resize does
        // not shuffle the figures around under the operator's eyes.
        for agent in self.room.agents() {
            if !rebuilt.admit(agent.clone()) {
                break;
            }
        }
        self.room = rebuilt;
        self.width = width;
        self.height = height;
    }

    /// Fold one transcript line into the room.
    ///
    /// Driven by the transcript rather than by raw events on purpose: the
    /// transcript is what both renderers read, so the office and the text can
    /// never disagree about what happened.
    pub fn feed(&mut self, line: &Line) {
        // Notices land in the daemon lane and do not need a figure — checking
        // for an agent first would drop them. The order matters: a daemon
        // event has no seat, so it must not be gated on having one.
        if let Line::Status(text) = line {
            self.room.touch_daemon("evolve", text, self.now_ms);
            return;
        }
        if matches!(line, Line::Reasoning(_)) {
            return;
        }

        // Every state change goes through `set_activity`, because that is the
        // one place that also stamps the clock. A direct write would leave a
        // stale `since_ms` and the doze timer would fire on the wrong state.
        let Some(id) = self.room.first_agent_id().map(str::to_string) else {
            return;
        };
        match line {
            Line::User(_) => {
                // A new message means the session is alive again. Without this
                // an agent that just received work would still show as dozing
                // for up to thirty seconds.
                self.room.set_activity(&id, Activity::Idle, self.now_ms);
            }
            Line::Assistant(_) | Line::Tool { .. } => {
                let detail = match line {
                    Line::Tool { name, .. } => name.clone(),
                    _ => String::new(),
                };
                self.room
                    .set_activity(&id, Activity::Working { detail }, self.now_ms);
            }
            Line::Error(_) | Line::Cancelled => {
                self.room.set_activity(&id, Activity::Idle, self.now_ms);
            }
            Line::Status(_) | Line::Reasoning(_) => {}
        }
    }

    /// Make sure the current session has a figure.
    pub fn ensure_session(&mut self, state: &TuiState) {
        if self
            .room
            .agents()
            .iter()
            .any(|agent| agent.id == state.session)
        {
            return;
        }
        self.room.admit(Agent {
            id: state.session.clone(),
            label: state.session.clone(),
            activity: if state.busy {
                Activity::Working {
                    detail: String::new(),
                }
            } else {
                Activity::Idle
            },
            since_ms: self.now_ms,
        });
    }

    /// Keep the busy flag honest: the gateway knows whether a turn is running,
    /// which is more reliable than inferring it from transcript contents.
    pub fn set_busy(&mut self, state: &TuiState) {
        if !state.busy {
            return;
        }
        let Some(id) = self.room.first_agent_id().map(str::to_string) else {
            return;
        };
        // Only promote from Idle. Overwriting a `Waiting` would hide the one
        // state that needs a human — the approval modal is up, and the office
        // must keep saying so.
        if matches!(self.room.agents()[0].activity, Activity::Idle) {
            self.room.set_activity(
                &id,
                Activity::Working {
                    detail: String::new(),
                },
                self.now_ms,
            );
        }
    }

    /// Render into a buffer.
    pub fn draw(&self, buffer: &mut Buffer, area: Rect) {
        scene::render(&self.room, buffer, area, self.now_ms, self.beat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn state_with(session: &str, busy: bool) -> TuiState {
        let mut state = TuiState::new(session.into(), None, "allow-read-only".into());
        state.busy = busy;
        state
    }

    #[test]
    fn a_session_gets_a_figure_exactly_once() {
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        let state = state_with("s-1", false);
        panel.ensure_session(&state);
        panel.ensure_session(&state);
        assert_eq!(panel.room().agents().len(), 1);
    }

    #[test]
    fn assistant_text_puts_the_agent_to_work() {
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.ensure_session(&state_with("s-1", false));
        panel.feed(&Line::Assistant("antwort".into()));
        assert!(
            matches!(panel.room().agents()[0].activity, Activity::Working { .. }),
            "got {:?}",
            panel.room().agents()[0].activity
        );
    }

    #[test]
    fn a_notice_becomes_a_daemon_not_an_agent() {
        // The daemon lane exists precisely because a notice has no session.
        // Folding it into the agent lane would show an evolve run as a seat
        // with nobody in it.
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.feed(&Line::Status("evolve 0007 verbessert".into()));
        assert!(panel.room().agents().is_empty());
        assert_eq!(panel.room().daemons().len(), 1);
        assert_eq!(panel.room().daemons()[0].detail, "evolve 0007 verbessert");
    }

    #[test]
    fn a_silent_daemon_is_swept_by_the_tick() {
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.tick(0);
        panel.feed(&Line::Status("läuft".into()));
        assert_eq!(panel.room().daemons().len(), 1);

        panel.tick(bkgclaw_office::room::DAEMON_SILENCE_MS + 1);
        assert!(
            panel.room().daemons().is_empty(),
            "a daemon that stopped talking must not stay on the floor"
        );
    }

    #[test]
    fn a_busy_flag_never_overwrites_a_waiting_agent() {
        // The one state that needs a human must survive the busy flag. The
        // gateway keeps `busy = true` while an approval is outstanding — the
        // turn has not ended, it is blocked — so promoting Waiting back to
        // Working would hide the one thing the operator has to act on.
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        let state = state_with("s-1", true);
        panel.ensure_session(&state);
        panel.room.set_activity(
            "s-1",
            Activity::Waiting {
                reason: "freigabe".into(),
            },
            panel.now_ms,
        );

        panel.set_busy(&state);
        assert!(
            matches!(panel.room().agents()[0].activity, Activity::Waiting { .. }),
            "busy must not mask the approval: {:?}",
            panel.room().agents()[0].activity
        );
        assert_eq!(
            panel.room.pose_for(0, panel.now_ms, 0),
            bkgclaw_office::room::Pose::Waiting
        );
    }

    #[test]
    fn a_resize_keeps_the_agents_and_replans_the_desks() {
        let mut panel = OfficePanel::new(80, OFFICE_HEIGHT);
        panel.ensure_session(&state_with("s-1", false));
        panel.ensure_session(&state_with("s-2", false));

        panel.resize(40, OFFICE_HEIGHT);
        assert_eq!(panel.room().agents().len(), 2, "nobody was dropped");
        // The room reports how many seats the new floor actually has, rather
        // than carrying the old count over — that is the number the operator
        // sees when the next session arrives.
        assert_eq!(
            panel.room().desks().len(),
            bkgclaw_office::room::Office::with_capacity(
                6,
                40 * bkgclaw_office::braille::Render::Braille.sub_width(),
                OFFICE_HEIGHT * bkgclaw_office::braille::Render::Braille.sub_height(),
            )
            .desks()
            .len(),
            "the floor was re-planned for the new width"
        );
    }

    #[test]
    fn a_resize_to_the_same_size_changes_nothing() {
        let mut panel = OfficePanel::new(60, OFFICE_HEIGHT);
        panel.ensure_session(&state_with("s-1", false));
        let before = panel.room().desks().len();
        panel.resize(60, OFFICE_HEIGHT);
        assert_eq!(panel.room().desks().len(), before);
    }

    #[test]
    fn the_busy_flag_moves_the_agent_to_working() {
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        let state = state_with("s-1", true);
        panel.ensure_session(&state);
        panel.set_busy(&state);
        assert!(matches!(
            panel.room().agents()[0].activity,
            Activity::Working { .. }
        ));
    }

    #[test]
    fn a_new_message_wakes_a_dozing_agent() {
        // The clock is the only thing that separates "finished" from "hung".
        // New work has to reset it immediately, or a resumed agent sleeps
        // through its own turn.
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.tick(0);
        panel.ensure_session(&state_with("s-1", false));
        panel.tick(bkgclaw_office::room::DOZE_AFTER_MS * 2);
        assert_eq!(
            panel.room.pose_for(0, panel.now_ms, 0),
            bkgclaw_office::room::Pose::Dozing
        );

        panel.feed(&Line::User("mach weiter".into()));
        panel.set_busy(&state_with("s-1", true));
        assert!(!matches!(
            panel.room.pose_for(0, panel.now_ms, 0),
            bkgclaw_office::room::Pose::Dozing
        ));
    }

    #[test]
    fn the_strip_actually_paints() {
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.ensure_session(&state_with("s-1", true));
        let area = Rect::new(0, 0, 40, OFFICE_HEIGHT);
        let mut buffer = Buffer::empty(area);
        panel.draw(&mut buffer, area);

        let painted = (0..40)
            .flat_map(|x| (0..OFFICE_HEIGHT).map(move |y| (x, y)))
            .filter(|(x, y)| buffer[(*x, *y)].fg != Color::Reset)
            .count();
        assert!(painted > 0, "the office strip is blank");
    }

    #[test]
    fn a_zero_height_strip_does_not_panic() {
        // A resize to nothing must not take the TUI down.
        let mut panel = OfficePanel::new(40, OFFICE_HEIGHT);
        panel.ensure_session(&state_with("s-1", true));
        let area = Rect::new(0, 0, 40, 0);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
        panel.draw(&mut buffer, area);
    }
}
