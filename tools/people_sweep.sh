#!/bin/zsh
# people_sweep.sh OUTDIR [GEN2] [SEEDS...] : people-plausibility sweep (gen2 --people 2 --mess 0.5 --qc, no glb) over seeds 5-8 of every room type,
# then tools/qc_report.py-style people summary. Default seeds 5 6 7 8. Rooms: Bedroom LivingRoom Kitchen Bathroom DiningRoom.
IG_TOOLS=${0:A:h}; . "$IG_TOOLS/env.sh"
OUT=${1:-$IG_ROOT/tmp/qcp}; mkdir -p $OUT
G=${2:-$IG_ROOT/target/fast/gen2}
seeds=(5 6 7 8); [[ $# -gt 2 ]] && seeds=(${@[3,-1]})
for room in Bedroom LivingRoom Kitchen Bathroom DiningRoom; do
  for seed in $seeds; do
    tag=${seed}_${room}
    $G $seed $room $OUT/x.glb --no-glb --rust-cloth --people 2 --mess 0.5 --qc $OUT/$tag.json >$OUT/$tag.log 2>&1; rc=$?
    [[ $rc -ne 0 ]] && echo "FAIL rc=$rc $tag"
  done
done
python3 $IG_TOOLS/people_report.py $OUT
