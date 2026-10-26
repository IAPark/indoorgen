#!/bin/zsh
# qc_place.sh [OUTDIR] [--gen2 PATH]  : placement audit (src/qc.rs) over ~20 scenes; prints per-scene defect counts + all issues.
# Env: NO_SNAP=1 disables the post-solve snap (the "before" numbers). Scenes: "seed room [gen2 flags]" lines in tools/qc_scenes.txt
IG_TOOLS=${0:A:h}; . "$IG_TOOLS/env.sh"
OUT=${1:-$IG_ROOT/tmp/qc}; mkdir -p $OUT
G=$IG_ROOT/target/fast/gen2
while read -r seed room flags; do
  [[ -z $seed || $seed == \#* ]] && continue
  tag=${seed}_${room}${flags:+_${${flags// /}//--/}}
  ${=G} $seed $room $OUT/x.glb --no-glb --qc $OUT/$tag.json ${=flags} >$OUT/$tag.log 2>&1; rc=$?
  [[ $rc -ne 0 ]] && echo "FAIL rc=$rc $tag"
  grep -E "^QC .* objects=" $OUT/$tag.log
done < $IG_TOOLS/qc_scenes.txt
python3 $IG_TOOLS/qc_report.py $OUT
