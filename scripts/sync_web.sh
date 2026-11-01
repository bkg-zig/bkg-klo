#!/usr/bin/env bash
# Build the Dioxus web cockpit and sync it into web-dist/ (BKGCLAW_WEB_DIR).
#
# web-dist/ is a hand-off directory, not a build artefact: it also holds the
# standalone office pages (chef.html, native.html, hive.html) and the pixel
# sprites (assets/manifest.json + assets/sprites/, made by
# scripts/gen_pixel_sprites.py). The sync therefore replaces ONLY what dx
# owns -- index.html and assets/bkgclaw-web* + assets/style-* -- and never
# deletes the hand-maintained files. "Sauber" means: after a sync, no stale
# hashed bundle from an older build lingers next to the fresh one.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
web="$repo/crates/bkgclaw-web"
dist="$web/target/dx/bkgclaw-web/release/web/public"
out="$repo/web-dist"

python3 "$repo/scripts/gen_pixel_sprites.py" >/dev/null

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  command -v dx >/dev/null || { echo "dx fehlt: cargo install dioxus-cli --version 0.7.10 --locked" >&2; exit 1; }
  (cd "$web" && dx build --release)
fi
[ -f "$dist/index.html" ] || { echo "kein dx-Output: $dist/index.html fehlt" >&2; exit 1; }

mkdir -p "$out/assets"
# Remove exactly the previous dx artefacts. The hashed names change per
# build AND per dx version (bkgclaw-web-dxh….js vs bkgclaw-web_bg-dxh….wasm),
# so match the whole family, not one pattern.
rm -f "$out"/assets/bkgclaw-web*.js "$out"/assets/bkgclaw-web*.wasm "$out"/assets/style-*.css

cp "$dist/index.html" "$out/index.html"
# dx accumulates every build's hashed bundles in its own public/assets and
# index.html references exactly one JS (its paired WASM is named inside
# that JS). Copy only the referenced pair + the CSS, never the whole dir,
# or stale bundles ride along forever.
js=$(grep -o 'assets/bkgclaw-web[^"]*\.js' "$dist/index.html" | head -1 | xargs -r basename)
[ -n "$js" ] || { echo "kein JS-Bundle in $dist/index.html referenziert" >&2; exit 1; }
wasm=$(grep -o 'assets/bkgclaw-web[^"]*\.wasm' "$dist/assets/$js" | head -1 | xargs -r basename)
[ -n "$wasm" ] || { echo "kein WASM in $js referenziert" >&2; exit 1; }
cp "$dist/assets/$js" "$dist/assets/$wasm" "$out/assets/"
for css in "$dist"/assets/style-*.css; do [ -e "$css" ] && cp "$css" "$out/assets/"; done

echo "web-dist synchron:"
ls -1 "$out" "$out/assets"
