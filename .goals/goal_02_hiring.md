# KI soll Mitarbeiter via opencode einstellen

Einstellen per `bkgclaw office hire --cli opencode --name OpenCode --desk desk_snes_01.png`.
Failover: claude-code fehlt/404 -> opencode (`bkgclaw_core::harness::resolve`).
Erfolg: `bkgclaw office list-employees` zeigt den Mitarbeiter, `cargo test -p bkgclaw-core harness` gruen.
