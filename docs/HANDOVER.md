# Übergabe (Handover) — bkgclaw / bkg-evolution

Stand: 1. November 2026.

## Kurzzustand

Der vollständige Rust-Workspace ist eingecheckt und auf die Remote gepusht.
Tests und Formatierung sind grün.
Die nächsten Schritte stehen in [Roadmap](ROADMAP.md) und [Aufgaben](TASKS.md).

## Repo

- Arbeitsverzeichnis: `/srv/bkg/services/bkg-evolution`
- Remote: `origin` → https://github.com/bkg-zig/bkg-klo.git
- Branch: `main`
- Letzter Commit: `0ab32c0` — `feat: bkgclaw-Workspace vollständig einchecken (ohne Secrets)`
- Davor: `aeeb8de` — `first commit` (nur README)

## Was eingecheckt ist

- Alle Crates unter `crates/`: `bkgclaw-core`, `-config`, `-store`, `-exec`, `-gateway`, `-tui`, `-web`, `-ui`, `-cli`, `-office`, `-sandbox` und `bkg-evolve`
- Ziele und Vorlagen unter `.goals/` (ohne `events.log`)
- Deploy: `Dockerfile`, `docker-compose.yml`, `railway.json`, `nixpacks.toml`, `server.js`
- Web-Auslieferung unter `web-dist/` inkl. 20 Pixel-Sprites
- Skripte: `scripts/gen_pixel_sprites.py`, `scripts/sync_web.sh`
- Fortschrittsnotiz: `progress.md` (älterer Stand, diese Docs hier sind aktueller)

## Was bewusst nicht im Repo ist

- `secrets/` mit `bkg_evolution_gateway_token.txt` und `bkg_evolution_nim_key.txt` — nur lokal, Rechte `600`, per `.gitignore` ausgeschlossen. Wer den Gateway braucht, legt die Dateien lokal an oder setzt `BKGCLAW_GATEWAY_TOKEN` und den NIM-Key als Umgebungsvariable.
- `.colabsync/` — lokale Sync-Konfiguration.
- `Cargo.lock` und `target/` — per `.gitignore` ausgeschlossen.
- `.goals/events.log` und alle `*.log`.

Achtung: Die Secrets wurden nie committed. Kein History-Rewrite nötig.

## Verifiziert in dieser Übergabe

- `git fetch` — Remote war aktuell, kein Pull nötig.
- `cargo test --workspace` — alle Tests grün (u. a. Core 191, Gateway-Integration, Office 3).
- `cargo fmt --check` — nach `cargo fmt` sauber.
- `git push origin main` — `aeeb8de..0ab32c0`, per `git ls-remote origin main` gegengeprüft.

Hinweis beim Push: Eine lokale Credential-Hilfsdatei meldet `cat: /run/secrets/gh_token: No such file or directory`.
Der Push lief trotzdem durch. Bei Gelegenheit die Git-Credential-Konfiguration prüfen.

## Offen und nicht verifiziert

- Kein PTY-Spawn: Hiring ist dry-run-sicher, startet keine echten Agent-Prozesse.
- Kein Live-HTTP-Smoke gegen den Gateway (nur Tests).
- `native.html` / `hive.html` nie im Browser geöffnet. WebGPU/Qwen2-0.5B unbewiesen.
- Kein Deploy ausgeführt (`railway up` fehlt).
- Im `README.md` stehen 314 Tests und „zero clippy warnings“ als Anspruch aus einer früheren Runde. In dieser Übergabe wurde nur `cargo test --workspace` und `cargo fmt --check` neu geprüft, kein Clippy-Lauf.

## So geht es weiter

1. Gateway lokal starten und HTTP-Smoke per `curl` nachholen.
2. PTY-Spawn für Hiring bauen.
3. Browser-Test und danach Railway-Deploy.
4. Details: [Aufgaben](TASKS.md), Reihenfolge: [Roadmap](ROADMAP.md).
