#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# gn_survey.sh LIST.txt OUT.log [SEED]   LIST: lines "module.Class" (module relative to infinigen.assets.objects); runs gn_check asset on each (sequential, one bpy at a time)
PY=$PY_REF
HERE=${0:A:h}
LIST=$1; OUT=$2; SEED=${3:-0}
: > $OUT
while read spec; do
  [ -z "$spec" ] && continue
  echo "### $spec" >> $OUT
  perl -e 'alarm 240; exec @ARGV' $PY $HERE/gn_check.py asset $spec $SEED 2>&1 | grep -v "OcMesher" | cut -c1-600 >> $OUT
done < $LIST
