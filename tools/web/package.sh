#!/bin/bash
# Package web/ into web/dist/ (static site) and web/dist/indoorgen.html (single self-contained file,
# JS inlined, worker + wasm embedded as base64). Needs node (esbuild is fetched via npx on first use).
# Usage: tools/web/package.sh [pkgdir]   pkgdir = wasm-bindgen output (default web/pkg)
set -e
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
W="$ROOT/web"; D="$W/dist"
PKG="${1:-$W/pkg}"
TMPDIR="${TMPDIR:-$ROOT/tmp}"; mkdir -p "$TMPDIR"
ESB="${ESBUILD:-$ROOT/tmp/tools/node_modules/.bin/esbuild}"
if [ ! -x "$ESB" ]; then (mkdir -p "$ROOT/tmp/tools" && cd "$ROOT/tmp/tools" && npm init -y >/dev/null && npm i esbuild >/dev/null 2>&1); fi
if [ "$PKG" != "$W/pkg" ] && [ -d "$PKG" ]; then rm -rf "$W/pkg"; mkdir -p "$W/pkg"; cp "$PKG"/ig_wasm.js "$PKG"/ig_wasm_bg.wasm "$W/pkg/"; fi

# (a) static site
rm -rf "$D"; mkdir -p "$D/site"
cp "$W/index.html" "$W"/*.js "$W/style.css" "$D/site/"
cp -R "$W/vendor" "$D/site/vendor"
[ -d "$W/pkg" ] && mkdir -p "$D/site/pkg" && cp "$W/pkg/ig_wasm.js" "$W/pkg/ig_wasm_bg.wasm" "$D/site/pkg/"
[ -d "$W/pkg-rt" ] && mkdir -p "$D/site/pkg-rt" && cp "$W/pkg-rt/preview.js" "$W/pkg-rt/preview_bg.wasm" "$D/site/pkg-rt/"

# (b) single file
ALIAS=(--alias:three="$W/vendor/three.module.js" --alias:three/addons="$W/vendor")
"$ESB" "$W/main.js" --bundle --format=iife --minify "${ALIAS[@]}" --outfile="$TMPDIR/main.bundle.js" --log-level=warning
if [ -f "$W/pkg/ig_wasm.js" ]; then
  "$ESB" "$W/worker.js" --bundle --format=iife --minify --outfile="$TMPDIR/worker.bundle.js" --log-level=error --define:import.meta.url='"http://localhost/"' --target=es2020
  base64 < "$W/pkg/ig_wasm_bg.wasm" | tr -d '\n' > "$TMPDIR/wasm.b64"
else
  echo "note: no wasm pkg; single file will be viewer-only" >&2
  echo "self.onmessage=()=>postMessage({type:'initfail',error:'wasm not built'})" > "$TMPDIR/worker.bundle.js"; : > "$TMPDIR/wasm.b64"
fi
base64 < "$TMPDIR/worker.bundle.js" | tr -d '\n' > "$TMPDIR/worker.b64"
python3 - "$W/index.html" "$W/style.css" "$TMPDIR/main.bundle.js" "$TMPDIR/worker.b64" "$TMPDIR/wasm.b64" "$D/indoorgen.html" <<'PY'
import sys,re
html,css,js,wkf,wasmf,out=sys.argv[1:7]
wk=open(wkf).read(); wasm=open(wasmf).read()
h=open(html).read()
h=re.sub(r'<script type="importmap">.*?</script>\s*','',h,flags=re.S)
h=h.replace('<link rel="stylesheet" href="style.css">','<style>'+open(css).read()+'</style>')
data='<script>window.__INDOORGEN={worker:"%s",wasm:"%s"};</script>'%(wk,wasm)
code=open(js).read().replace('</script','<\\/script')
h=h.replace('<script type="module" src="main.js"></script>',data+'\n<script>'+code+'</script>')
open(out,'w').write(h)
PY
echo "== sizes"; du -sh "$D/site" ; ls -l "$D/indoorgen.html" | awk '{printf "indoorgen.html %.1f MB\n",$5/1e6}'
[ -f "$W/pkg/ig_wasm_bg.wasm" ] && ls -l "$W/pkg/ig_wasm_bg.wasm" | awk '{printf "wasm %.1f MB\n",$5/1e6}'
exit 0
