# syntax=docker/dockerfile:1.7
# bkgclaw: the gateway with its web cockpit, plus the evolve daemon.
#
#   docker compose up --build
#   open http://127.0.0.1:8788      → cockpit (token in /srv/bkg/services/bkgclaw/secrets)
#   docker compose logs -f evolve   # the self-improvement loop, live
#
# Two stages. The browser bundle and the server binary come out of one tree,
# but only the binary is a normal Linux program: the cockpit is WASM and is
# *served* by the gateway, never executed here.
#
# The runtime stage keeps the full Rust toolchain on purpose: the evolve
# daemon's fitness gate runs `cargo test` and `cargo clippy` inside the
# container, so an image without cargo would make every cycle fail its gate
# and park every goal — a daemon that can never accept its own work.

ARG RUST_VERSION=1.98.1
ARG UID=1000
ARG GID=1000

# ---- build: the native binary --------------------------------------------------
# The browser bundle is NOT built here. dx 0.7.10 runs on the host (matching
# the workspace lockfile) and install.sh copies its output into
# deploy/web-dist/ before this image builds — compiling the CLI's ~800
# crates inside a throwaway layer once filled the disk, and a host tool
# that is already verified beats a second toolchain to babysit.
FROM rust:${RUST_VERSION}-trixie AS build
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates curl git pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
# Manifests first: dependency compilation then survives a source-only change,
# which is the difference between a slow and a slower rebuild.
COPY Cargo.toml Cargo.lock ./
COPY crates crates

# The native binary. `--locked` so a drifting lockfile fails the build rather
# than silently pulling a different dependency graph into a shipped image.
RUN --mount=type=cache,target=/src/target \
    cargo build --release --locked -p bkgclaw-cli \
 && cp target/release/bkgclaw /usr/local/bin/bkgclaw

# ---- runtime ------------------------------------------------------------------
# The rust base, not a slim one: the fitness gate needs cargo inside the
# container. `git` for the evolve daemon's branches, snapshots and pushes;
# `curl` for the healthcheck; `ca-certificates` for HTTPS to the NIM gateway.
FROM rust:${RUST_VERSION}-trixie
ARG UID
ARG GID
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        ca-certificates curl git \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --gid "${GID}" bkg \
 && useradd --uid "${UID}" --gid "${GID}" --create-home --shell /bin/bash bkg

COPY --from=build /usr/local/bin/bkgclaw /usr/local/bin/bkgclaw
# The cockpit is served as static files from this directory — built on the
# host by install.sh (see the build stage's note for why).
COPY deploy/web-dist /srv/bkgclaw/web
# A full checkout with history: the evolve daemon clones its own working tree
# from this on first start, so the daemon gets a real git repo — branches,
# snapshots, a base to reset to — without ever touching the host checkout.
COPY . /srv/bkgclaw/src
COPY deploy/evolve-start.sh /usr/local/bin/evolve-start.sh
RUN chmod +x /usr/local/bin/evolve-start.sh \
 && mkdir -p /var/lib/bkgclaw /var/bkgclaw-evolve-tree \
 && chown -R bkg:bkg /var/lib/bkgclaw /var/bkgclaw-evolve-tree /srv/bkgclaw

USER bkg
# `/usr/local/cargo/bin` first: the rust image's default PATH has it, and
# overriding PATH without it made the fitness gate's `cargo` unfindable —
# every cycle measured "cargo nicht ausführbar" and reverted.
ENV BKGCLAW_HOME=/var/lib/bkgclaw \
    BKGCLAW_WEB_DIR=/srv/bkgclaw/web \
    PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin

WORKDIR /srv/bkgclaw/src
EXPOSE 8788
# No default CMD: compose gives each service its own command, and a stray
# `docker run` without one should fail loudly rather than start a daemon
# nobody asked for.
CMD ["bkgclaw", "--help"]
