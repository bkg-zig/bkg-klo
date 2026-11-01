# bkg-evolution — Fortschritt (Master Prompt)

Stand: Office-CLI fertig, Rest verdrahtet. Kein Electron, 100 % nativ im Web.

## Fertig und getestet

| Aufgabe | Stand | Nachweis |
|---|---|---|
| 01 Büro (`crates/bkgclaw-office`) | 20x14, 5 Desks, Tankstelle-Stationen | `cargo test -p bkgclaw-office`: 3 Tests grün |
| 02 Hiring + Failover | `office hire/list-employees`, Katalog in Core | Akzeptanzlauf: `eingestellt: OpenCode`, `1 mitarbeiter` |
| 03 WebMCP + WebLLM (Rust-Seite) | 9 Tools über REST + WebSocket, Gate für Destruktives | Gateway-Tests: 12 + 8 Integrationstests grün |
| 04 Self-Evolution | `.goals/` lesen, `self-evolve` listet, `--run` evolviert | Akzeptanzlauf: `3 proposals` |
| 05 Railway-Dateien | `railway.json`, `nixpacks.toml`, `Dockerfile`, `server.js` | `node --check` ok, JSON ok |

Voller Lauf `cargo test -p bkgclaw-core -p bkgclaw-office -p bkg-evolve -p bkgclaw-gateway -p bkgclaw-cli -p bkgclaw-tui` war grün (u. a. Office 3, TUI 12).

## Neu in dieser Runde

- **Failover:** `crates/bkgclaw-core/src/harness.rs` — 11 Harness-CLIs, `claude-code → opencode`, auflösbar auch bei fehlender Binary.
- **WebLLM-Vertrag:** `crates/bkgclaw-core/src/orc_webllm.rs` — Qwen2-0.5B + Fallbacks, System-Prompt, Intent-Routing, JSON-Tool-Calls.
- **Tankstelle:** `FloorTask` mit Abhängigkeit — B startet erst, wenn A `Done` ist (tanken vor bezahlen).
- **Gateway:** `GET /api/office|floor|agents`, `POST /api/office/tools/{name}`, Events `office_tool_call` / `office_updated`, `$PORT` für Railway.
- **Web (statisch):** `web-dist/native.html` (Hire + Canvas-Lerp, pixelated) und `web-dist/hive.html` (WebMCP + WebLLM mit Mock-Fallback).
- **Ziele:** `.goals/goal_01_office_autonomy.md`, `.goals/goal_02_hiring.md`, Befehl `bkgclaw self-evolve`.
- **Pixel-Sprites (echt, SNES):** `scripts/gen_pixel_sprites.py` erzeugt alle 20 Assets aus `web-dist/assets/manifest.json` (+`envelope_01.png`) als RGBA-PNGs (Desks 64x48, Agents 24x32 3/4-View, Rack 40x64) — Outline #04101f, Shading-Ramps, Neon-Code-Bars; `chef.html`/`native.html` laden sie aus `assets/sprites/` und fallen ohne PNG auf Canvas-Farbfelder zurück. Envelope-Packets wandern in `chef.html` sichtbar Desk→Desk.
- **Web-Cockpit-Build:** `dx build --release` (dioxus-cli 0.7.10) in `crates/bkgclaw-web`; `./scripts/sync_web.sh` synct nur das von `index.html` referenzierte JS/WASM-Paar + CSS nach `web-dist/` (dx sammelt alte Hash-Bundles im eigenen Output; `cp -r` hätte sie mitgeschleppt) und behält Office-Pages/Sprites. Routing: `/` = Dioxus-Cockpit, `/office` = `chef.html`; Dockerfile-Builder baut das Cockpit per `SKIP_BUILD=0 ./scripts/sync_web.sh` mit.

## Offen / nicht verifiziert

- **Kein PTY-Spawn:** Hire pflegt nur Floor-State (dry-run-sicher); echte `opencode run …`-Prozesse startet noch nichts.
- **HTTP-Smoke fehlt:** Gateway per `curl` auf `/api/health` + `/api/office/tools/*` wurde zweimal per Approval abgelehnt — Routen sind per Unit-/Integrationstest abgedeckt, nicht per Live-Request.
- **Browser ungetestet:** `native.html` / `hive.html` nie im Browser geöffnet; WebGPU-Laden von Qwen2-0.5B unbewiesen (Mock-Fallback im Code).
- **Keine 113 PNGs:** `web-dist/assets/` enthält nur Build-JS/CSS plus `manifest.json`; fehlende SNES-Bilder rendert das Canvas als Farbfelder.
- **Kein Deploy:** Kein `railway up` ausgeführt; Startbefehl im `railway.json` zeigt auf Port 8787, Gateway ehrt zusätzlich `$PORT`.
- **Format offen:** `cargo fmt --check` wurde per Approval abgelehnt.

## Ausprobieren

```bash
cargo run -p bkgclaw-cli -- office hire --cli opencode --name "OpenCode" --desk desk_snes_01.png
cargo run -p bkgclaw-cli -- office list-employees
cargo run -p bkgclaw-cli -- self-evolve
cargo run -p bkgclaw-cli -- gateway   # dann /native.html und /hive.html öffnen
```
