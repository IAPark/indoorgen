#!/bin/zsh
# dens_bench.sh OUTDIR [ROOMS...] : density + time for target-room-only gen2 over seeds 5-8 (env passes through: STEPS_L/M/S, INFINIGEN_PROGRAM)
IG_TOOLS=${0:A:h}; . "$IG_TOOLS/env.sh"
OUT=${1:?outdir}; shift; mkdir -p $OUT
rooms=($@); (( $# )) || rooms=(Bedroom LivingRoom DiningRoom Kitchen Bathroom)
for r in $rooms; do for s in 5 6 7 8; do
  st=$(date +%s)
  $IG_ROOT/target/fast/gen2 $s $r $OUT/x.glb --no-glb --qc $OUT/${s}_$r.json >$OUT/${s}_$r.log 2>&1; rc=$?
  echo "$r $s rc=$rc t=$(( $(date +%s) - st ))s" 
done; done | tee $OUT/times.txt
python3 $IG_TOOLS/dens_report.py $OUT
