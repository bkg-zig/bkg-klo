# Roadmap — bkgclaw / bkg-evolution

Stand: 1. November 2026. Remote: `origin/main` (github.com/bkg-zig/bkg-klo).

## Wo wir stehen

Der Rust-Workspace ist vollständig eingecheckt und gepusht.
`cargo test --workspace` ist grün, `cargo fmt --check` ist sauber.
Secrets liegen nur lokal in `secrets/` und sind per `.gitignore` ausgeschlossen.

Fertig und getestet:

- Kern: Agent-Loop, Modelle, Routing, Kosten, Leak-Scan (`crates/bkgclaw-core`)
- Gateway: REST + WebSocket, Sessions, Approvals, Sub-Agenten (`crates/bkgclaw-gateway`)
- CLI, TUI und Web-Cockpit (Dioxus/WASM) (`crates/bkgclaw-cli`, `-tui`, `-web`)
- Office: 20x14-Büro, 5 Desks, Tankstellen-Stationen, Hiring per CLI (`crates/bkgclaw-office`)
- Self-Evolution: `.goals/` lesen, `self-evolve` listet Vorschläge, `--run` evolviert (`crates/bkg-evolve`)
- Deploy-Dateien: `Dockerfile`, `docker-compose.yml`, `railway.json`, `nixpacks.toml`, `server.js`

## Nächste Schritte (Priorität)

1. **Echte Prozesse statt nur Floor-State:** Hire startet heute keine PTY-Prozesse.
   Ziel: `opencode run …` wirklich spawnen, Status zurückmelden.
2. **Live-HTTP-Smoke:** Gateway starten und `/api/health` sowie `/api/office/tools/*` per `curl` prüfen.
   Bisher nur per Unit- und Integrationstest abgedeckt.
3. **Browser-Test:** `web-dist/native.html` und `web-dist/hive.html` im Browser öffnen.
   WebGPU-Laden von Qwen2-0.5B ist unbewiesen, Mock-Fallback liegt im Code.
4. **Deploy:** `railway up` ausführen und den Start auf Port 8787 bzw. `$PORT` live verifizieren.
5. **Sprites vervollständigen:** Aktuell 20 echte SNES-PNGs in `web-dist/assets/sprites/`.
   Fehlende Bilder rendert das Canvas als Farbfelder.

## Später

- Failover-Kette der 11 Harness-CLIs (`claude-code → opencode`) unter realem Ausfall testen.
- WebMCP + WebLLM ohne Mock-Fallback Ende-zu-Ende nachweisen.
- `Cargo.lock` einchecken, sobald der Workspace als Binary-Release ausgeliefert wird (aktuell per `.gitignore` ausgeschlossen).

## Nicht-Ziele

- Kein Electron. Alles bleibt nativ bzw. Web.
- Keine Secrets im Repo. Tokens nur über `secrets/` lokal oder Umgebungsvariablen.
