#!/bin/zsh
# meshqc_batch.sh OUTDIR : generate the QC scene set (20 scenes, all room types, plain and --people 2 --mess 0.5), run tools/meshqc.py on each,
# write OUTDIR/<name>.json, delete the glb. Then `python tools/meshqc_sum.py OUTDIR` for the per-defect / per-class table.
IG_TOOLS=${0:A:h}; . "$IG_TOOLS/env.sh"
OUT=${1:?outdir}; mkdir -p $OUT; T=${TMPDIR:-/tmp}
GEN=${GEN:-$IG_ROOT/target/fast/gen2}
scenes=(
 "Kitchen 1" "Kitchen 3" "Kitchen 5" "LivingRoom 1" "LivingRoom 2" "LivingRoom 4"
 "Bedroom 1" "Bedroom 2" "Bedroom 3" "Bathroom 1" "Bathroom 2" "Bathroom 3" "DiningRoom 1" "DiningRoom 2"
 "Kitchen 2 P" "LivingRoom 3 P" "Bedroom 4 P" "Bathroom 4 P" "DiningRoom 3 P" "Kitchen 4 P"
)
for s in $scenes; do
  w=(${=s}); room=$w[1]; seed=$w[2]; extra=(); nm=${room}_$seed
  [[ $w[3] == P ]] && extra=(--people 2 --mess 0.5) && nm=${nm}_pm
  [[ -f $OUT/$nm.json ]] && continue
  $GEN $seed $room $T/qc_$nm.glb $extra >$T/qc_$nm.log 2>&1 || { echo "GEN FAIL $nm rc=$?"; }
  [[ -f $T/qc_$nm.glb ]] && $PY_REF $IG_TOOLS/meshqc.py $T/qc_$nm.glb --json $OUT/$nm.json --top 0 >/dev/null
  rm -f $T/qc_$nm.glb $T/qc_$nm.json
  echo "done $nm"
done
