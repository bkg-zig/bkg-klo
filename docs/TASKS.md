# Aufgaben — bkgclaw / bkg-evolution

Stand: 1. November 2026. Erledigte Punkte mit Nachweis, offene Punkte in Reihenfolge.

## Erledigt

- [x] Workspace sauber eingecheckt (149 Dateien, Commit `0ab32c0`) und auf `origin/main` gepusht
- [x] Secrets (`secrets/`), `.colabsync/` und `*.log` per `.gitignore` ausgeschlossen und aus dem Index entfernt
- [x] `cargo fmt` angewendet, `cargo fmt --check` sauber
- [x] `cargo test --workspace` grün
- [x] Office-CLI fertig: Büro 20x14, 5 Desks, Tankstellen-Stationen — Nachweis: `cargo test -p bkgclaw-office` (3 Tests grün)
- [x] Hiring + Failover: `office hire/list-employees`, Katalog in Core — Akzeptanzlauf: `eingestellt: OpenCode`
- [x] WebMCP + WebLLM (Rust-Seite): 9 Tools über REST + WebSocket, Gate für Destruktives — Gateway-Tests grün
- [x] Self-Evolution: `.goals/` lesen, `self-evolve` listet 3 Vorschläge, `--run` evolviert
- [x] Railway-Dateien geprüft: `node --check` ok, JSON ok
- [x] Pixel-Sprites erzeugt: 20 RGBA-PNGs per `scripts/gen_pixel_sprites.py`, Fallback auf Canvas-Farbfelder
- [x] Web-Cockpit-Build per `scripts/sync_web.sh` nach `web-dist/` synchronisiert

## Offen

- [ ] PTY-Spawn: Hire pflegt nur Floor-State (dry-run-sicher). Echte `opencode run …`-Prozesse startet noch nichts
- [ ] HTTP-Smoke: Gateway per `curl` auf `/api/health` + `/api/office/tools/*` live prüfen (bisher nur Tests)
- [ ] Browser-Test: `native.html` / `hive.html` im Browser öffnen, WebGPU-Laden von Qwen2-0.5B nachweisen
- [ ] Deploy: `railway up` ausführen, Port 8787 bzw. `$PORT` live verifizieren
- [ ] Sprites: fehlende SNES-Bilder erzeugen, bis kein Canvas-Fallback mehr nötig ist

## Ausprobieren

```bash
cargo run -p bkgclaw-cli -- office hire --cli opencode --name "OpenCode" --desk desk_snes_01.png
cargo run -p bkgclaw-cli -- office list-employees
cargo run -p bkgclaw-cli -- self-evolve
cargo run -p bkgclaw-cli -- gateway   # dann /native.html und /hive.html öffnen
```
