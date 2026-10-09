#!/usr/bin/env bash
# Start the bkgclaw stack with a token.
#
#   ./deploy/start.sh            # build, start, print the URL
#   ./deploy/start.sh --rebuild  # force a fresh image
#   ./deploy/start.sh --stop     # stop and keep the data
#
# Why a script at all: the token has to exist before compose starts, or the
# gateway boots unauthenticated. Generating it here — once, on disk, with
# 0600 — means the stack can never come up open by accident, which is a
# different guarantee from "remember to set the variable".

set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(cd .. && pwd)"
DATA="$ROOT/deploy/data"
ENV_FILE="$DATA/.env"
TOKEN_FILE="$DATA/gateway-token"

mkdir -p "$DATA"

case "${1:-}" in
  --stop)
    docker compose stop
    echo "gestoppt. daten bleiben in $DATA"
    exit 0
    ;;
  --rebuild)
    docker compose build --no-cache
    ;;
  "") ;;
  *)
    echo "unbekanntes argument: $1" >&2
    echo "nutzung: $0 [--rebuild|--stop]" >&2
    exit 2
    ;;
esac

# The token is generated once and then kept. Regenerating it on every start
# would invalidate open browser tabs for no reason; reusing an existing file
# means a restart is invisible to anyone already looking at the cockpit.
if [[ ! -s "$TOKEN_FILE" ]]; then
  # openssl if there is one, /dev/urandom otherwise — this script cannot
  # assume a toolchain is installed just to generate 32 bytes.
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32 > "$TOKEN_FILE"
  else
    LC_ALL=C tr -dc 'a-f0-9' < /dev/urandom | head -c 64 > "$TOKEN_FILE"
  fi
  chmod 600 "$TOKEN_FILE"
  echo "neu: gateway-token erzeugt"
fi

TOKEN="$(cat "$TOKEN_FILE")"

# Compose reads BKGCLAW_GATEWAY_TOKEN from the environment or its own env file.
# A file, not a shell export, so `docker compose up` by hand later gets the
# same token instead of coming up open.
{
  echo "BKGCLAW_GATEWAY_TOKEN=${TOKEN}"
  # Pass the model key through if it is in the environment; never write it to
  # disk here — a compose env file is a file, and a file gets backed up.
  if [[ -n "${NIM_API_KEY:-}" ]]; then
    echo "NIM_API_KEY=${NIM_API_KEY}"
  fi
} > "$ENV_FILE"
chmod 600 "$ENV_FILE"

docker compose up -d --build

PORT="${BKGCLAW_PORT:-8788}"
echo
echo "  cockpit   http://127.0.0.1:${PORT}/?token=${TOKEN}"
echo "  api       http://127.0.0.1:${PORT}/api/health"
echo "  evolve    docker compose logs -f evolve"
echo "  tail      tail -f $DATA/../data/gateway-token >/dev/null  # nur zum Prüfen"
echo
echo "  token liegt in: $TOKEN_FILE (0600)"
echo "  daten liegen in: $DATA"