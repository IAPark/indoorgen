#!/bin/bash
# smoke.sh [BIN]: generate one small room twice; check determinism, glb magic and sidecar json.
# Golden md5 (glb+json) is only checked with INDOORGEN_GOLDEN=1: it matches builds WITH OpenVDB (voxel remesh) only.
set -e
BIN=${1:-target/fast/indoorgen}; W=$(mktemp -d); trap 'rm -rf $W' EXIT
"$BIN" 0 Bathroom $W/a.glb > /dev/null 2> $W/a.log
"$BIN" 0 Bathroom $W/b.glb > /dev/null 2>&1
cmp $W/a.glb $W/b.glb && cmp $W/a.json $W/b.json && echo "deterministic"
[ "$(head -c4 $W/a.glb)" = "glTF" ] && echo "glb magic ok"
python3 - $W/a.json <<'PY'
import json,sys
d=json.load(open(sys.argv[1])); n=len(d["nodes"]); print("sidecar nodes:", n); assert n>=20, n
PY
if [ -n "$INDOORGEN_GOLDEN" ]; then
  got=$(cat $W/a.glb $W/a.json | python3 -c 'import sys,hashlib;print(hashlib.md5(sys.stdin.buffer.read()).hexdigest())')
  want=$(cut -d' ' -f1 "$(dirname "$0")/golden_bathroom0.md5")
  [ "$got" = "$want" ] && echo "golden md5 ok" || { echo "golden md5 MISMATCH $got vs $want"; exit 1; }
fi
