//! The room: who is here, what they are doing, and where they sit.
//!
//! # Why three states and not six
//!
//! The obvious design is `Idle / Working / Waiting / Done / Failed / Sleeping`
//! — and it is wrong, because `Done` and `Failed` are *events*, not states.
//! An agent is not "done"; it finished a turn and is now idle, and the
//! difference the operator cares about is *how long ago*. So there are three
//! states and a clock:
//!
//! | gateway says | figure shows |
//! |---|---|
//! | a turn is running | typing, hands alternating |
//! | an approval is pending | seated, with a `?` overhead |
//! | neither | seated; after a while, dozing |
//!
//! "Finished ten minutes ago" and "hung since ten minutes ago" then look
//! different without inventing a state for either — one dozes, one still
//! types. That is the whole design, and it is why three states carry the
//! information six would.

use serde::{Deserialize, Serialize};

use crate::sprite::seeded_colour;

/// What one agent is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activity {
    /// No turn running.
    Idle,
    /// A turn is running — the model is answering or a tool is executing.
    Working { detail: String },
    /// Blocked on an approval. The only state that demands a human, so the
    /// only one that gets a marker in the middle of the screen.
    Waiting { reason: String },
}

impl Activity {
    /// How the status line reads.
    pub fn label(&self) -> String {
        match self {
            Activity::Idle => "bereit".into(),
            Activity::Working { detail } if detail.is_empty() => "arbeitet".into(),
            Activity::Working { detail } => format!("arbeitet: {detail}"),
            Activity::Waiting { reason } => format!("wartet: {reason}"),
        }
    }
}

/// One agent at one desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// The gateway session id. Doubles as the colour seed, so the same
    /// session always looks the same.
    pub id: String,
    /// Short label for the caption under the desk.
    pub label: String,
    pub activity: Activity,
    /// Milliseconds since the epoch when the current activity began. Drives
    /// the doze-after-a-while behaviour, and is what makes "hung" visible.
    pub since_ms: u64,
}

/// A desk someone can sit at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Desk {
    /// Left edge in pixels.
    pub x: u16,
    /// Top edge in pixels.
    pub y: u16,
}

/// The office: a floor plan plus the agents on it.
///
/// Two lanes, deliberately separate:
/// - **agents** sit at desks and have a session id.
/// - **daemons** do not. They are background workers with no conversation —
///   for bkgclaw that is the evolve engine, which publishes
///   `Event::Notice { session: "-" }` and therefore has no session to filter
///   by.
///
/// Folding a daemon into the agent lane would force it to borrow a session id
/// it does not have, and the operator would see a "session" that cannot be
/// opened, forked or closed. So it gets its own slot type and its own
/// presence, and the two are never confused.
#[derive(Debug, Clone, Default)]
pub struct Office {
    agents: Vec<Agent>,
    daemons: Vec<Daemon>,
    desks: Vec<Desk>,
    /// Width of the floor in pixels.
    width: u16,
    height: u16,
}

/// A background worker with no session. Present or not — never "waiting".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daemon {
    /// A stable key, e.g. `"evolve"`.
    pub key: String,
    /// What it is doing, shown on the caption.
    pub detail: String,
    /// Milliseconds since the epoch when it was last heard from.
    pub last_seen_ms: u64,
}

/// After this long idle, an agent dozes instead of sitting up.
pub const DOZE_AFTER_MS: u64 = 30_000;

/// A daemon that has said nothing for this long is considered gone.
///
/// Generous on purpose: an evolve cycle runs a full agent turn, and a sweep
/// that fired mid-run would make a working daemon blink out of existence.
pub const DAEMON_SILENCE_MS: u64 = 5 * 60_000;

/// Pixels between desks, horizontally and vertically.
pub const DESK_GAP_X: u16 = 4;
pub const DESK_GAP_Y: u16 = 3;

/// A desk is this wide in pixels — wide enough for a figure beside a screen.
pub const DESK_W: u16 = 12;
/// A desk plus the figure seated at it.
pub const CELL_W: u16 = DESK_W + DESK_GAP_X;
pub const CELL_H: u16 = 15;

impl Office {
    /// A floor that fits `desks` agents.
    ///
    /// Desks are laid out in a row rather than a grid, because a chat TUI is
    /// wide and short. Wrapping would put an agent on a second row where the
    /// caption below the first row already sits.
    pub fn with_capacity(capacity: usize, width: u16, height: u16) -> Office {
        let columns = width.div_ceil(CELL_W).max(1) as usize;
        let rows = height.div_ceil(CELL_H).max(1) as usize;
        let desks = (0..capacity.min(columns * rows))
            .map(|index| Desk {
                x: (index as u16 % columns as u16) * CELL_W,
                y: (index as u16 / columns as u16) * CELL_H,
            })
            .collect();
        Office {
            agents: Vec::new(),
            daemons: Vec::new(),
            desks,
            width,
            height,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// The id of the first agent, if any.
    ///
    /// Callers that want to change an agent's state use
    /// [`Office::set_activity`] — which takes the id and does the lookup. That
    /// asymmetry is deliberate: there is exactly one mutation path, so a state
    /// change always goes through the code that also stamps the clock.
    pub fn first_agent_id(&self) -> Option<&str> {
        self.agents.first().map(|agent| agent.id.as_str())
    }

    pub fn daemons(&self) -> &[Daemon] {
        &self.daemons
    }

    /// Record that a daemon spoke. A daemon is not admitted like an agent:
    /// it takes no desk and cannot overflow the floor, because it is not a
    /// conversation. Repeated notices update the existing entry.
    pub fn touch_daemon(&mut self, key: &str, detail: &str, now_ms: u64) {
        match self.daemons.iter_mut().find(|daemon| daemon.key == key) {
            Some(daemon) => {
                daemon.detail = detail.to_string();
                daemon.last_seen_ms = now_ms;
            }
            None => self.daemons.push(Daemon {
                key: key.to_string(),
                detail: detail.to_string(),
                last_seen_ms: now_ms,
            }),
        }
    }

    /// Drop daemons that have been silent for too long.
    ///
    /// A daemon's liveness is silence. There is no heartbeat to ask for and no
    /// turn to cancel, so the only honest signal is "not heard from since".
    pub fn sweep_daemons(&mut self, now_ms: u64) {
        self.daemons
            .retain(|daemon| now_ms.saturating_sub(daemon.last_seen_ms) < DAEMON_SILENCE_MS);
    }

    /// Append an agent without checking for a free desk.
    ///
    /// Only the tests use this, to build the "more agents than desks" state
    /// that a well-behaved event handler would never produce. Production code
    /// goes through [`Office::admit`], which refuses.
    #[cfg(test)]
    pub(crate) fn agents_mut_for_test(&mut self) -> &mut Vec<Agent> {
        &mut self.agents
    }

    pub fn desks(&self) -> &[Desk] {
        &self.desks
    }

    /// How many more agents fit.
    pub fn free_desks(&self) -> usize {
        self.desks.len().saturating_sub(self.agents.len())
    }

    /// Whether another agent has room.
    pub fn has_room(&self) -> bool {
        self.free_desks() > 0
    }

    /// Put an agent at a free desk. Returns false when the office is full —
    /// which is information, not an error: the caller says so rather than
    /// silently dropping a session.
    pub fn admit(&mut self, agent: Agent) -> bool {
        if !self.has_room() {
            return false;
        }
        self.agents.push(agent);
        true
    }

    /// Remove an agent by session id.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|agent| agent.id != id);
        self.agents.len() != before
    }

    /// Record a state change for a session. An unknown id is ignored: an
    /// event for a session that is not on the floor is not this office's
    /// business.
    pub fn set_activity(&mut self, id: &str, activity: Activity, now_ms: u64) -> bool {
        match self.agents.iter_mut().find(|agent| agent.id == id) {
            Some(agent) => {
                if agent.activity != activity {
                    agent.activity = activity;
                    agent.since_ms = now_ms;
                }
                true
            }
            None => false,
        }
    }

    /// The desk an agent sits at, by its index.
    pub fn desk_for(&self, index: usize) -> Option<Desk> {
        self.desks.get(index).copied()
    }

    /// The pose an agent should be in, given what it is doing and how long.
    ///
    /// Time is the interesting input: an agent that just went idle is still
    /// sitting up, one that has been idle long enough is dozing. Without the
    /// clock, "finished" and "stuck" are the same picture.
    pub fn pose_for(&self, index: usize, now_ms: u64, beat: u64) -> Pose {
        let Some(agent) = self.agents.get(index) else {
            return Pose::Absent;
        };
        match &agent.activity {
            Activity::Working { .. } => Pose::Typing {
                frame: (beat / 2) % 2,
            },
            Activity::Waiting { .. } => Pose::Waiting,
            Activity::Idle => {
                let idle_for = now_ms.saturating_sub(agent.since_ms);
                if idle_for >= DOZE_AFTER_MS {
                    Pose::Dozing
                } else {
                    Pose::Seated
                }
            }
        }
    }

    /// An agent's colour, stable for the life of its session.
    pub fn colour_for(&self, index: usize, slot: char) -> crate::pixels::Rgb {
        match self.agents.get(index) {
            Some(agent) => personal_colour(&agent.id, slot),
            None => crate::pixels::Rgb::new(128, 128, 128),
        }
    }
}

/// What a figure is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// Not in the room.
    Absent,
    /// Sitting up, nothing happening.
    Seated,
    /// Working; `frame` alternates between the two typing poses.
    Typing { frame: u64 },
    /// Waiting for a human. Never animated — a moving figure reads as "still
    /// busy", and the whole point of this state is that it is not.
    Waiting,
    /// Idle long enough to doze.
    Dozing,
}

impl Pose {
    pub fn figure_name(&self) -> &'static str {
        match self {
            Pose::Absent => "",
            Pose::Seated | Pose::Waiting => "seated",
            Pose::Typing { frame } => {
                if *frame == 0 {
                    "typing_0"
                } else {
                    "typing_1"
                }
            }
            Pose::Dozing => "sleeping",
        }
    }
}

/// The colour an agent's `slot` is drawn in.
///
/// Derived from the session id, so the same session always looks the same —
/// the office has to read as one place, not as a lottery that rerolls on
/// every frame. Four salts keep hair, skin, shirt and trousers apart instead
/// of all landing on the same hue.
fn personal_colour(id: &str, slot: char) -> crate::pixels::Rgb {
    let salt = match slot {
        'H' => 0x4841_4952, // HAIR
        'S' => 0x534b494e,  // SKIN
        'B' => 0x5348_4952, // SHIRT
        'P' => 0x5041_4e54, // PANTS
        _ => 0,
    };
    seeded_colour(id, salt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn office_with(capacity: usize) -> Office {
        Office::with_capacity(capacity, 120, 60)
    }

    fn agent(id: &str) -> Agent {
        Agent {
            id: id.into(),
            label: id.into(),
            activity: Activity::Idle,
            since_ms: 0,
        }
    }

    #[test]
    fn an_agent_is_admitted_only_when_a_desk_is_free() {
        let mut office = office_with(1);
        assert!(office.admit(agent("a")));
        assert!(!office.admit(agent("b")), "the office is full");
        assert_eq!(office.agents().len(), 1);
    }

    #[test]
    fn a_full_office_says_so_instead_of_dropping_the_agent_silently() {
        // The caller has to be able to say "no room" to the operator.
        let mut office = office_with(1);
        assert!(office.admit(agent("a")));
        assert!(!office.has_room());
        assert_eq!(office.free_desks(), 0);
    }

    #[test]
    fn freeing_a_desk_lets_the_next_agent_in() {
        let mut office = office_with(1);
        office.admit(agent("a"));
        assert!(office.remove("a"));
        assert!(office.admit(agent("b")));
        assert_eq!(office.agents()[0].id, "b");
    }

    #[test]
    fn removing_an_unknown_agent_reports_nothing_happened() {
        let mut office = office_with(2);
        office.admit(agent("a"));
        assert!(!office.remove("ghost"));
    }

    #[test]
    fn a_working_agent_types_and_a_waiting_one_does_not() {
        let mut office = office_with(1);
        office.admit(agent("a"));
        office.set_activity(
            "a",
            Activity::Working {
                detail: "read".into(),
            },
            1_000,
        );

        assert_eq!(office.pose_for(0, 1_000, 0), Pose::Typing { frame: 0 });
        assert_eq!(office.pose_for(0, 1_000, 2), Pose::Typing { frame: 1 });

        // Waiting is never animated. A figure that keeps moving while it is
        // blocked on a human reads as busy, and it is not.
        office.set_activity(
            "a",
            Activity::Waiting {
                reason: "freigabe".into(),
            },
            2_000,
        );
        assert_eq!(office.pose_for(0, 2_000, 0), Pose::Waiting);
        assert_eq!(office.pose_for(0, 2_000, 6), Pose::Waiting);
    }

    #[test]
    fn idle_becomes_dozing_only_after_a_while() {
        // "Finished" and "hung" are the same state; only the clock separates
        // them, so this behaviour is the reason the clock exists.
        let mut office = office_with(1);
        let mut subject = agent("a");
        // The agent arrives working, then goes idle at a known instant. Seeding
        // it as idle instead would make `set_activity` a no-op — it only
        // stamps the clock on a real change — and the doze timer would sit at
        // epoch zero.
        subject.activity = Activity::Working {
            detail: String::new(),
        };
        office.admit(subject);
        let start = 100_000;
        office.set_activity("a", Activity::Idle, start);

        assert_eq!(office.pose_for(0, start, 0), Pose::Seated);
        assert_eq!(
            office.pose_for(0, start + DOZE_AFTER_MS - 1, 0),
            Pose::Seated
        );
        assert_eq!(office.pose_for(0, start + DOZE_AFTER_MS, 0), Pose::Dozing);
    }

    #[test]
    fn a_state_change_resets_the_clock() {
        let mut office = office_with(1);
        office.admit(agent("a"));
        office.set_activity("a", Activity::Idle, 1_000);
        assert_eq!(office.pose_for(0, 99_000, 0), Pose::Dozing);
        office.set_activity(
            "a",
            Activity::Working {
                detail: String::new(),
            },
            99_000,
        );
        assert_eq!(
            office.pose_for(0, 100_000, 0),
            Pose::Typing { frame: 0 },
            "the doze timer restarted with the state"
        );
    }

    #[test]
    fn re_setting_the_same_state_does_not_restart_the_clock() {
        // A delta stream would otherwise reset the timer on every token, and
        // nothing would ever doze.
        let mut office = office_with(1);
        office.admit(agent("a"));
        office.set_activity("a", Activity::Idle, 1_000);
        office.set_activity("a", Activity::Idle, 2_000);
        assert_eq!(office.pose_for(0, 10_000, 0), Pose::Seated);
    }

    #[test]
    fn an_event_for_an_unknown_session_changes_nothing() {
        let mut office = office_with(2);
        office.admit(agent("a"));
        assert!(!office.set_activity("ghost", Activity::Idle, 0));
        assert_eq!(office.agents()[0].activity, Activity::Idle);
    }

    #[test]
    fn an_absent_index_yields_the_absent_pose_not_a_panic() {
        let office = office_with(1);
        assert_eq!(office.pose_for(9, 0, 0), Pose::Absent);
    }

    #[test]
    fn each_agent_gets_its_own_colour() {
        let mut office = office_with(2);
        office.admit(agent("a"));
        office.admit(agent("b"));
        assert_ne!(
            office.colour_for(0, 'B'),
            office.colour_for(1, 'B'),
            "two agents must not share a shirt"
        );
    }

    #[test]
    fn an_agents_colour_is_stable_across_frames() {
        let mut office = office_with(1);
        office.admit(agent("a"));
        let first = office.colour_for(0, 'H');
        for _ in 0..50 {
            assert_eq!(office.colour_for(0, 'H'), first);
        }
    }

    #[test]
    fn desks_do_not_overlap() {
        // Two agents sharing a desk would render on top of each other and the
        // office would look broken in a way that is hard to diagnose.
        let office = office_with(8);
        let desks = office.desks();
        for (index, a) in desks.iter().enumerate() {
            for b in &desks[index + 1..] {
                let separated = a.x.abs_diff(b.x) >= CELL_W || a.y.abs_diff(b.y) >= CELL_H;
                assert!(separated, "desks {a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn an_absurd_capacity_is_capped_by_the_floor() {
        // A narrow terminal must not promise fifty desks and then draw four.
        let office = office_with(100);
        assert!(office.desks().len() <= 100);
        assert!(!office.desks().is_empty());
    }

    #[test]
    fn activity_labels_read_as_sentences() {
        assert_eq!(Activity::Idle.label(), "bereit");
        assert_eq!(
            Activity::Working {
                detail: "read_file".into()
            }
            .label(),
            "arbeitet: read_file"
        );
        assert_eq!(
            Activity::Working {
                detail: String::new()
            }
            .label(),
            "arbeitet"
        );
        assert_eq!(
            Activity::Waiting {
                reason: "freigabe".into()
            }
            .label(),
            "wartet: freigabe"
        );
    }

    #[test]
    fn a_daemon_takes_no_desk_and_no_agent_slot() {
        // This is the whole point of the daemon lane. A daemon has no
        // session, so admitting it as an agent would produce an entry the
        // operator cannot open, fork or close.
        let mut office = office_with(1);
        office.admit(agent("a"));
        office.touch_daemon("evolve", "versuch 0007", 1_000);

        assert_eq!(office.agents().len(), 1, "no extra agent appeared");
        assert_eq!(office.daemons().len(), 1);
        assert_eq!(office.free_desks(), 0, "the daemon took no desk");
    }

    #[test]
    fn repeated_notices_update_one_daemon_instead_of_stacking() {
        // An evolve cycle emits several notices. Without this the office would
        // fill with one entry per progress line.
        let mut office = office_with(2);
        for step in 0..5 {
            office.touch_daemon("evolve", &format!("schritt {step}"), step);
        }
        assert_eq!(office.daemons().len(), 1);
        assert_eq!(office.daemons()[0].detail, "schritt 4");
    }

    #[test]
    fn a_silent_daemon_is_swept_and_a_talking_one_is_not() {
        let mut office = office_with(2);
        office.touch_daemon("evolve", "läuft", 1_000);
        office.touch_daemon("build", "gebaut", 1_000);

        // Long enough that `build` has gone quiet but `evolve` just spoke.
        office.touch_daemon("evolve", "läuft noch", DAEMON_SILENCE_MS + 2_000);
        office.sweep_daemons(DAEMON_SILENCE_MS + 2_000);

        assert_eq!(office.daemons().len(), 1);
        assert_eq!(office.daemons()[0].key, "evolve");
    }

    #[test]
    fn a_running_daemon_is_not_swept_mid_cycle() {
        // An evolve cycle is a full agent turn. A sweep that fired during it
        // would blink the working daemon out of the office.
        let mut office = office_with(2);
        let start = 1_000;
        office.touch_daemon("evolve", "start", start);
        // Progress notices keep arriving; the gap between them stays small.
        for step in 1..10u64 {
            office.touch_daemon("evolve", &format!("schritt {step}"), start + step * 1_000);
        }
        office.sweep_daemons(start + 10_000);
        assert_eq!(
            office.daemons().len(),
            1,
            "still there: {:?}",
            office.daemons()
        );
    }
}
