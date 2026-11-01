FROM rust:1.85 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release
# Das Web-Cockpit (Dioxus 0.7 -> WASM) wird VOR dem Image-Build per
# ./scripts/sync_web.sh nach web-dist/ synchronisiert und reist im
# Build-Kontext mit. Ein dx-Neubau im Image (cargo install dioxus-cli)
# hat den Build auf dieser Maschine per ENOSPC gesprengt, deshalb hier
# nur der harte Nachweis: Das in index.html referenzierte JS/WASM-Paar
# muss im Kontext liegen, sonst bricht der Build ab statt eine
# Statusseite als / auszuliefern.
RUN js=$(grep -o 'assets/bkgclaw-web[^"]*\.js' web-dist/index.html | head -1) \
 && test -n "$js" && test -f "web-dist/$js" \
 && wasm=$(grep -o 'assets/bkgclaw-web[^"]*\.wasm' "web-dist/$js" | head -1) \
 && test -n "$wasm" && test -f "web-dist/$wasm" \
 && echo "web-dist ok: $js + $wasm"
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/bkgclaw /usr/local/bin/bkgclaw
COPY --from=builder /app/web-dist /app/web-dist
ENV BKGCLAW_WEB_DIR=/app/web-dist
EXPOSE 8787
CMD ["bkgclaw", "gateway", "--host", "0.0.0.0", "--port", "8787"]
