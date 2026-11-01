use anyhow::Result;
use serde::{Deserialize, Serialize};

// Agent-office rendering ported from upstream bkgclaw: every running
// session as a figure at a desk (room model, figures, pixel/Braille
// renderers). Namespaced under modules so it does not clash with the
// floor/harness `Office` below. Terminal renderers need ratatui and
// sit behind the `terminal` feature.
#[cfg(feature = "terminal")]
pub mod braille;
pub mod figure;
pub mod pixels;
pub mod portrait;
pub mod room;
#[cfg(feature = "terminal")]
pub mod scene;
pub mod sprite;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Office {
    pub width: usize,
    pub height: usize,
    pub desks: Vec<Desk>,
    pub employees: Vec<Employee>,
    #[serde(default)]
    pub tasks: Vec<FloorTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Desk {
    pub x: usize,
    pub y: usize,
    pub asset: String,
    pub station_type: StationType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StationType {
    CodePump,
    TestShop,
    ReviewCounter,
    DeployPump,
    Break,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub id: String,
    pub name: String,
    pub harness_cli: String,
    pub sprite: String,
    pub desk: Option<(usize, usize)>,
    pub status: EmployeeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmployeeStatus {
    GreenActive,
    AmberAttention,
    BlueReady,
    RedError,
}

/// Harness CLIs that can be hired. The catalogue lives in
/// `bkgclaw_core::harness` (single source); this re-export keeps the office
/// API stable. Unknown CLIs are rejected so a typo never silently becomes a
/// celebrate-sprite employee.
pub use bkgclaw_core::harness::HARNESS_CLIS;
pub const SUPPORTED_CLIS: &[&str] = &[
    "opencode",
    "claude-code",
    "codex",
    "grok",
    "kimi",
    "qwen",
    "copilot",
    "cursor",
    "antigravity",
    "crush",
    "pi",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

/// Tankstelle task: B needs A like tanken then bezahlen. `can_start` is the
/// gate; a task whose dependency is not `Done` never starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FloorTask {
    pub id: String,
    pub title: String,
    pub station: StationType,
    pub depends_on: Option<String>,
    pub status: TaskStatus,
    pub assignee: Option<String>,
}

impl FloorTask {
    pub fn can_start(&self, all: &[FloorTask]) -> bool {
        match &self.depends_on {
            None => true,
            Some(dep) => all
                .iter()
                .find(|t| &t.id == dep)
                .map(|t| t.status == TaskStatus::Done)
                .unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub id: String,
    pub from: String,
    pub to: String,
    pub text: String,
}

pub struct OfficeHarness {
    pub office: Office,
}

impl OfficeHarness {
    pub fn new() -> Self {
        Self {
            office: Office {
                width: 20,
                height: 14,
                desks: Vec::new(),
                employees: Vec::new(),
                tasks: Vec::new(),
            },
        }
    }

    pub async fn setup_office(&mut self, w: usize, h: usize) -> Result<()> {
        self.setup_office_sync(w, h)
    }

    pub fn setup_office_sync(&mut self, w: usize, h: usize) -> Result<()> {
        self.office.width = w;
        self.office.height = h;
        // Platzhalter für Floor/Tile Logik
        self.office.desks.clear();
        // Beispiel-Desks gemäß Spec
        self.office.desks.push(Desk {
            x: 2,
            y: 3,
            asset: "desk_dual_monitor_01.png".into(),
            station_type: StationType::CodePump,
        });
        self.office.desks.push(Desk {
            x: 5,
            y: 3,
            asset: "desk_snes_01.png".into(),
            station_type: StationType::CodePump,
        });
        self.office.desks.push(Desk {
            x: 8,
            y: 5,
            asset: "server_rack_snes_01.png".into(),
            station_type: StationType::TestShop,
        });
        self.office.desks.push(Desk {
            x: 12,
            y: 7,
            asset: "cabinet_01.png".into(),
            station_type: StationType::ReviewCounter,
        });
        self.office.desks.push(Desk {
            x: 15,
            y: 10,
            asset: "plant_snes_01.png".into(),
            station_type: StationType::Break,
        });
        Ok(())
    }

    pub async fn hire_employee(
        &mut self,
        cli: &str,
        name: &str,
        desk_asset: &str,
    ) -> Result<Employee> {
        self.hire_employee_sync(cli, name, desk_asset)
    }

    pub fn hire_employee_sync(
        &mut self,
        cli: &str,
        name: &str,
        desk_asset: &str,
    ) -> Result<Employee> {
        let Some(spec) = bkgclaw_core::harness::harness(cli) else {
            anyhow::bail!(
                "unsupported harness cli {cli:?} (supported: {})",
                SUPPORTED_CLIS.join(", ")
            );
        };
        let sprite = spec.sprite;
        let desk = self
            .office
            .desks
            .iter()
            .find(|d| d.asset == desk_asset)
            .map(|d| (d.x, d.y));
        let id = format!("emp_{}", self.office.employees.len() + 1);
        let emp = Employee {
            id: id.clone(),
            name: name.into(),
            harness_cli: cli.into(),
            sprite: sprite.into(),
            desk,
            status: EmployeeStatus::BlueReady,
        };
        self.office.employees.push(emp.clone());
        // PTY spawn happens in the caller (gateway/CLI gate); this harness
        // only owns floor state so `cargo run -- office hire` stays dry-run safe.
        Ok(emp)
    }

    /// Tankstelle flow: one envelope per occupied desk, desk-to-desk.
    pub fn send_envelope(&self, from: &str, to: &str, text: &str) -> Envelope {
        Envelope {
            id: format!("env_{from}_{to}"),
            from: from.to_string(),
            to: to.to_string(),
            text: text.to_string(),
        }
    }

    /// Parallel floor pass: every employee hands work to the next station.
    pub fn run_floor_envelopes(&self) -> Vec<Envelope> {
        let order = [
            StationType::CodePump,
            StationType::TestShop,
            StationType::ReviewCounter,
            StationType::DeployPump,
            StationType::Break,
        ];
        let mut out = Vec::new();
        for emp in &self.office.employees {
            let Some((x, y)) = emp.desk else {
                continue;
            };
            let Some(stage) = self
                .office
                .desks
                .iter()
                .find(|d| (d.x, d.y) == (x, y))
                .map(|d| d.station_type.clone())
            else {
                continue;
            };
            let idx = order.iter().position(|s| *s == stage).unwrap_or(0);
            let next = &order[(idx + 1) % order.len()];
            if let Some(target) = self.office.desks.iter().find(|d| d.station_type == *next) {
                out.push(self.send_envelope(
                    &format!("{}@{x},{y}", emp.id),
                    &format!("desk@{x},{y}", x = target.x, y = target.y),
                    &format!("handoff from {}", emp.name),
                ));
            }
        }
        out
    }

    /// WebMCP `place_desk` / `place_chair`: place or replace at (x, y).
    pub fn place_desk(
        &mut self,
        x: usize,
        y: usize,
        asset: &str,
        station: StationType,
    ) -> Result<Desk> {
        if x >= self.office.width || y >= self.office.height {
            anyhow::bail!(
                "desk at {x},{y} outside {}x{}",
                self.office.width,
                self.office.height
            );
        }
        self.office.desks.retain(|d| (d.x, d.y) != (x, y));
        let desk = Desk {
            x,
            y,
            asset: asset.into(),
            station_type: station,
        };
        self.office.desks.push(desk.clone());
        Ok(desk)
    }

    /// WebMCP `move_agent`: lerp target is the desk; state snaps, page animates.
    pub fn move_agent(&mut self, employee_id: &str, x: usize, y: usize) -> Result<Employee> {
        let Some(emp) = self
            .office
            .employees
            .iter_mut()
            .find(|e| e.id == employee_id)
        else {
            anyhow::bail!("no employee {employee_id}");
        };
        emp.desk = Some((x, y));
        Ok(emp.clone())
    }

    pub fn set_agent_status(
        &mut self,
        employee_id: &str,
        status: EmployeeStatus,
    ) -> Result<Employee> {
        let Some(emp) = self
            .office
            .employees
            .iter_mut()
            .find(|e| e.id == employee_id)
        else {
            anyhow::bail!("no employee {employee_id}");
        };
        emp.status = status;
        Ok(emp.clone())
    }

    /// WebMCP `assign_task` with Tankstelle dependency gate.
    pub fn assign_task(&mut self, task: FloorTask) -> Result<FloorTask> {
        if let Some(dep) = &task.depends_on {
            let ready = self
                .office
                .tasks
                .iter()
                .find(|t| &t.id == dep)
                .map(|t| t.status == TaskStatus::Done)
                .unwrap_or(false);
            if !ready {
                anyhow::bail!(
                    "task {} blocked: dependency {dep} not done (tanken vor bezahlen)",
                    task.id
                );
            }
        }
        self.office.tasks.push(task.clone());
        Ok(task)
    }

    pub async fn run_floor(&self) -> Result<()> {
        let _ = self.run_floor_envelopes();
        Ok(())
    }

    pub fn export_level(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.office)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hire_assigns_the_named_desk() {
        let mut h = OfficeHarness::new();
        h.setup_office(20, 14).await.unwrap();
        let emp = h
            .hire_employee("opencode", "OpenCode", "desk_snes_01.png")
            .await
            .unwrap();
        assert_eq!(emp.desk, Some((5, 3)));
        assert_eq!(emp.sprite, "agent_walking_right_01.png");
        assert!(
            h.hire_employee("emacs", "X", "desk_snes_01.png")
                .await
                .is_err()
        );
        let envs = h.run_floor_envelopes();
        assert_eq!(envs.len(), 1);
    }

    #[test]
    fn tankstelle_dependency_blocks_until_done() {
        let mut h = OfficeHarness::new();
        let a = FloorTask {
            id: "A".into(),
            title: "tanken".into(),
            station: StationType::CodePump,
            depends_on: None,
            status: TaskStatus::Pending,
            assignee: None,
        };
        let b = FloorTask {
            id: "B".into(),
            title: "bezahlen".into(),
            station: StationType::ReviewCounter,
            depends_on: Some("A".into()),
            status: TaskStatus::Pending,
            assignee: None,
        };
        h.office.tasks.push(a);
        assert!(!b.can_start(&h.office.tasks));
        assert!(h.assign_task(b.clone()).is_err());
        h.office.tasks[0].status = TaskStatus::Done;
        assert!(b.can_start(&h.office.tasks));
        assert!(h.assign_task(b).is_ok());
        h.place_desk(3, 4, "desk_dual_monitor_01.png", StationType::CodePump)
            .unwrap();
        assert!(h.place_desk(99, 4, "x.png", StationType::Break).is_err());
    }

    #[tokio::test]
    async fn test_office_setup() {
        let mut harness = OfficeHarness::new();
        harness.setup_office(20, 14).await.unwrap();
        assert_eq!(harness.office.width, 20);
        assert_eq!(harness.office.height, 14);
        assert!(!harness.office.desks.is_empty());
    }
}
