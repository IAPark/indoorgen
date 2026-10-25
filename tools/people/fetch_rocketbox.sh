#!/bin/bash
# Fetch Microsoft Rocketbox avatars (MIT) into ~/work/assets/rocketbox/<name>/ : Export/<name>.fbx + textures (TGA -> jpg/png, TGA deleted).
# usage: fetch_rocketbox.sh [name ...]   (default: the curated list below). Needs python with PIL (infinigen-ref venv).
set -e
OUT=${RB_OUT:-$HOME/work/assets/rocketbox}
PY=${RB_PY:-$HOME/work/infinigen-ref/.venv/bin/python}
HERE=$(cd "$(dirname "$0")" && pwd)
BASE=https://raw.githubusercontent.com/microsoft/Microsoft-Rocketbox/master/Assets/Avatars
LIST="Female_Adult_01 Female_Adult_02 Female_Adult_04 Female_Adult_05 Female_Adult_07 Female_Adult_09 Female_Adult_11 Female_Adult_14 Female_Adult_17
Male_Adult_01 Male_Adult_03 Male_Adult_05 Male_Adult_07 Male_Adult_09 Male_Adult_11 Male_Adult_06 Male_Adult_13 Male_Adult_17 Male_Adult_20
Business_Female_01 Business_Male_01 Female_Adult_13 Female_Adult_15 Male_Adult_02 Male_Adult_08"
[ $# -gt 0 ] && LIST="$*"
mkdir -p "$OUT"
[ -f "$OUT/tree.json" ] || curl -s "https://api.github.com/repos/microsoft/Microsoft-Rocketbox/git/trees/master?recursive=1" -o "$OUT/tree.json"
for n in $LIST; do
  [ -f "$OUT/$n/.done" ] && continue
  cat="$($PY -I -c "
import json,sys
for e in json.load(open('$OUT/tree.json'))['tree']:
    p=e['path']
    if p.endswith('/Export/$n.fbx'): print(p.split('/')[2])
")"
  [ -z "$cat" ] && { echo "no such avatar $n"; continue; }
  mkdir -p "$OUT/$n"
  files="$($PY -I -c "
import json
for e in json.load(open('$OUT/tree.json'))['tree']:
    p=e['path']
    if p.startswith('Assets/Avatars/$cat/$n/') and e['type']=='blob' and '_facial' not in p and 'wrinkle' not in p and ' ' not in p.split('/Textures/')[-1] and not p.endswith(('.shader','.mat','.meta')): print(p.split('/$n/')[1])
")"
  for f in $files; do
    mkdir -p "$OUT/$n/$(dirname $f)"
    curl -sfL "$BASE/$cat/$n/$f" -o "$OUT/$n/$f"
  done
  $PY -I "$HERE/rb_convert.py" "$OUT/$n/Textures"
  touch "$OUT/$n/.done"
  echo "$n ($cat) $(du -sk "$OUT/$n" | cut -f1) KB"
done
cat > "$OUT/LICENSE-ATTRIBUTION.txt" <<'EOT'
Microsoft Rocketbox Avatar Library, https://github.com/microsoft/Microsoft-Rocketbox
Copyright (c) Microsoft Corporation. Released under the MIT License (see the repository LICENSE).
Gonzalez-Franco et al., "The Rocketbox Library and the Utility of Freely Available Rigged Avatars", Frontiers in Virtual Reality, 2020.
Textures here were re-encoded from TGA to JPEG/PNG; the FBX files are unmodified.
EOT
