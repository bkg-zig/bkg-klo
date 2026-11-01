//! `bkgclaw office` — the self-arranged floor (Task 01/02 of the master prompt).
//!
//! Thin dispatch over `bkgclaw_office::OfficeHarness`. State persists as
//! JSON in the workspace root (`office.json`) so `office hire` in one
//! process is visible to `office list-employees` in the next.

use crate::outcome::Outcome;
use bkgclaw_core::Verdict;
use bkgclaw_office::OfficeHarness;

fn office_path() -> std::path::PathBuf {
    bkgclaw_store::workspace_root().join("office.json")
}

fn load() -> Result<OfficeHarness, String> {
    let path = office_path();
    if !path.exists() {
        return Ok(OfficeHarness::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let office: bkgclaw_office::Office = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(OfficeHarness { office })
}

fn save(harness: &OfficeHarness) -> Result<(), String> {
    let path = office_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = harness.export_level().map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

pub async fn setup(width: usize, height: usize) -> Outcome {
    let mut h = match load() {
        Ok(h) => h,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    if let Err(e) = h.setup_office(width, height).await {
        return Outcome::fail(Verdict::environment(e.to_string()));
    }
    if let Err(e) = save(&h) {
        return Outcome::fail(Verdict::environment(e));
    }
    Outcome::success(format!(
        "office eingerichtet: {}x{} mit {} desks -> {}",
        width,
        height,
        h.office.desks.len(),
        office_path().display()
    ))
}

pub async fn hire(cli: &str, name: &str, desk: &str) -> Outcome {
    let mut h = match load() {
        Ok(h) => h,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    if h.office.desks.is_empty() {
        if let Err(e) = h
            .setup_office(h.office.width.max(20), h.office.height.max(14))
            .await
        {
            return Outcome::fail(Verdict::environment(e.to_string()));
        }
    }
    let emp = match h.hire_employee(cli, name, desk).await {
        Ok(e) => e,
        Err(e) => {
            let msg = e.to_string();
            if msg.starts_with("unsupported harness") {
                return Outcome::fail(Verdict::usage(msg));
            }
            return Outcome::fail(Verdict::environment(msg));
        }
    };
    if let Err(e) = save(&h) {
        return Outcome::fail(Verdict::environment(e));
    }
    Outcome::success(format!(
        "eingestellt: {} ({}) -> {}",
        emp.name, emp.harness_cli, emp.sprite
    ))
}

pub fn list_employees() -> Outcome {
    let h = match load() {
        Ok(h) => h,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    if h.office.employees.is_empty() {
        return Outcome::from_report(bkgclaw_ui::Report::negative(
            "noch keine mitarbeiter",
            "einstellen mit: bkgclaw office hire --cli opencode --name OpenCode --desk desk_snes_01.png",
        ));
    }
    let checks = h
        .office
        .employees
        .iter()
        .map(|e| bkgclaw_ui::Check {
            name: format!("{} · {}", e.id, e.name),
            ok: matches!(
                e.status,
                bkgclaw_office::EmployeeStatus::BlueReady
                    | bkgclaw_office::EmployeeStatus::GreenActive
            ),
            code: "[EMP]".into(),
            detail: format!("{} · {} · desk {:?}", e.harness_cli, e.sprite, e.desk),
        })
        .collect();
    Outcome::from_report(bkgclaw_ui::Report::with_checks(
        format!("{} mitarbeiter", h.office.employees.len()),
        checks,
    ))
}

pub fn export() -> Outcome {
    let h = match load() {
        Ok(h) => h,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    print_level(&h)
}

fn print_level(h: &OfficeHarness) -> Outcome {
    match h.export_level() {
        Ok(json) => Outcome::from_report(
            bkgclaw_ui::Report::ok("floor export").with_data(serde_json::json!({ "level": json })),
        ),
        Err(e) => Outcome::fail(Verdict::environment(e.to_string())),
    }
}

pub async fn run() -> Outcome {
    let h = match load() {
        Ok(h) => h,
        Err(e) => return Outcome::fail(Verdict::environment(e)),
    };
    let envs = h.run_floor_envelopes();
    if let Err(e) = h.run_floor().await {
        return Outcome::fail(Verdict::environment(e.to_string()));
    }
    Outcome::success(format!(
        "floor laeuft: {} employees, {} envelopes (tankstelle)",
        h.office.employees.len(),
        envs.len()
    ))
}
