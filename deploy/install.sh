#!/usr/bin/env bash
# Install the bkgclaw stack to its canonical service location and start it.
#
#   ./deploy/install.sh           # install, build, start, print the URL
#   ./deploy/install.sh --stop    # stop; data and secrets stay
#
# Idempotent: re-running refreshes the gh token, re-copies the compose file
# and rebuilds the image.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
SERVICE_DIR=/srv/bkg/services/bkgclaw
SECRETS="$SERVICE_DIR/secrets"
COMPOSE="$SERVICE_DIR/docker-compose.yml"

mkdir -p "$SECRETS"

# The gateway token: generated once, then kept. Regenerating on every
# install would invalidate every open browser tab for no reason.
if [ ! -s "$SECRETS/gateway_token.txt" ]; then
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32 > "$SECRETS/gateway_token.txt"
  else
    LC_ALL=C tr -dc 'a-f0-9' < /dev/urandom | head -c 64 > "$SECRETS/gateway_token.txt"
  fi
  chmod 600 "$SECRETS/gateway_token.txt"
  echo "neu: gateway-token erzeugt"
fi

# The GitHub token: refreshed on every install. gh may have rotated it, and
# a stale token means the daemon's improvements stop arriving on the remote
# while the changelog still says everything is fine.
gh auth token > "$SECRETS/gh_token.txt"
chmod 600 "$SECRETS/gh_token.txt"

# The browser bundle comes from the host's dx — the tool is version-matched
# to the workspace lockfile and already verified; building it inside the
# image once filled the disk with a second toolchain for no benefit.
command -v dx >/dev/null 2>&1 || {
  echo "dx fehlt: cargo install dioxus-cli --version 0.7.10 --locked" >&2
  exit 1
}
echo "web-bundle bauen (dx auf dem host)…"
(cd "$REPO/crates/bkgclaw-web" && dx build --release >/dev/null)
rm -rf "$REPO/deploy/web-dist"
cp -r "$REPO/crates/bkgclaw-web/target/dx/bkgclaw-web/release/web/public" "$REPO/deploy/web-dist"

# The compose definition is versioned in the repository; the service
# directory holds the live copy, next to its secrets.
cp "$REPO/deploy/docker-compose.yml" "$COMPOSE"

case "${1:-}" in
  --stop)
    docker compose -f "$COMPOSE" stop
    echo "gestoppt. daten und secrets bleiben in $SERVICE_DIR"
    exit 0
    ;;
  "") ;;
  *)
    echo "unbekanntes argument: $1" >&2
    echo "nutzung: $0 [--stop]" >&2
    exit 2
    ;;
esac

# Syntax and secret wiring first — a compose that references a missing
# secret must fail here, not at 3 a.m. in a restart loop.
docker compose -f "$COMPOSE" config --quiet

docker compose -f "$COMPOSE" up -d --build

TOKEN="$(cat "$SECRETS/gateway_token.txt")"
echo
echo "  cockpit   http://127.0.0.1:8788/?token=${TOKEN}"
echo "  api       http://127.0.0.1:8788/api/health"
echo "  evolve    docker compose -f $COMPOSE logs -f evolve"
echo "  token     $SECRETS/gateway_token.txt (0600)"
