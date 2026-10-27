#!/bin/zsh
# usage: tools/rules_matrix.sh ROOM "Class Class..." SPEC.json|none seeds...   -> per-seed object counts (gen2 --only-target)
cd "$(dirname $0)/.."; room=$1; classes=$2; spec=$3; shift 3
for s in "$@"; do
  a=(); [[ $spec != none ]] && a=(--rules $spec)
  target/fast/gen2 $s $room ${TMPDIR:-/tmp}/rm.glb --only-target $a >${TMPDIR:-/tmp}/rm.log 2>&1; rc=$?
  echo "seed $s $(basename $spec) rc=$rc $(python3 tools/rules_count.py ${TMPDIR:-/tmp}/rm.json ${=classes})"
done
rm -f ${TMPDIR:-/tmp}/rm.glb
