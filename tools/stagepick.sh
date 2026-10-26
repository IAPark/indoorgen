#!/bin/bash
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# stagepick.sh RS_DIR PY_DIR FROM TO [tol]: stagediff restricted to stage numbers FROM..TO
cd $IG_ROOT; A=$1; B=$2; mkdir -p tmp/_pa tmp/_pb; for f in tmp/_pa/* tmp/_pb/*; do [ -L "$f" ] && rm "$f"; done
for n in $(seq $3 $4); do p=$(printf "%03d" $n); for f in $A/${p}_*_out.bin; do [ -e "$f" ] && ln -sf $PWD/$f tmp/_pa/; done; for f in $B/${p}_*_out.bin; do [ -e "$f" ] && ln -sf $PWD/$f tmp/_pb/; done; done
target/fast/stagediff tmp/_pa tmp/_pb ${5:-1e-4}
