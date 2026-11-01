#!/bin/sh
# The evolve daemon's first minutes, every start.
#
# One image, two lives: the gateway runs from the image as-is, the daemon
# needs a *working tree of its own* — a git repo it may commit to, branch in
# and `git reset --hard` on failure. The image carries a full checkout with
# history; the daemon clones it into its volume on first start and never
# looks back at the host checkout. A reset inside this tree can reach exactly
# this tree and nothing else — which is the "kann nix kaputt machen" the
# operator asked for.
set -eu

TREE=/var/bkgclaw-evolve-tree
HOME_DIR=/var/lib/bkgclaw

# --- git identity: commits need an author, and "evolve" is the honest one.
git config --global user.name "bkgclaw-evolve"
git config --global user.email "evolve@bkgclaw.local"

# --- GitHub push credentials: read from the mounted secret at use time.
# Written as a credential helper rather than into a remote URL, so the token
# never lands in .git/config — a file that gets backed up, copied and read.
git config --global credential.helper \
  '!f() { echo username=soniaschu; echo "password=$(cat /run/secrets/gh_token)"; }; f'

# --- the daemon's own tree, cloned once from the image checkout.
if [ ! -d "$TREE/.git" ]; then
  echo "evolve: erster start — klone den arbeitsbaum aus dem image" >&2
  git clone --quiet --no-hardlinks /srv/bkgclaw/src "$TREE"
  git -C "$TREE" remote set-url origin https://github.com/soniaschu/bkg-evolution.git
fi

cd "$TREE"

# Never sit on a protected branch: the daemon refuses to cycle on main or
# master, and a fresh clone starts wherever the image was built from. The
# `evolution` branch is the daemon's base — its cycles branch off it.
if git show-ref --verify --quiet refs/heads/evolution; then
  git switch --quiet evolution
else
  git switch --quiet -c evolution
fi

# A fresh stack seeds one goal so it is visibly doing something; a daemon
# that starts and waits is indistinguishable from one that is broken. The
# queue file lives in the shared volume — once it exists, this never fires
# again and the operator's queue is the only queue.
if [ ! -f "$HOME_DIR/evolve/queue.jsonl" ]; then
  bkgclaw evolve seed \
    "suche im tui die groesste offene baustelle und schliesse sie mit einem test" \
    || true
fi

# `--push`: an improved branch nobody can see is a budget spent for nothing.
# The token comes from the credential helper above; a refused push lands in
# the changelog, not in a silent loss.
exec bkgclaw evolve daemon --push
