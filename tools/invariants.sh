#!/bin/zsh
# invariants.sh [OUTDIR]: the invariant checks (src/invariants.rs, INVARIANTS=1) over the regression seeds, one row per seed + totals.
# Env: SEEDS="3 6 ..." to override, CFG=<config json> (default: features on, cloth off, no people), EXTRA="<env assignments>" (e.g. HIER=1)
OUT=${1:-$HOME/work/bench/inv}; mkdir -p $OUT
G=$HOME/work/infinigen-rs/target/fast/gen2
CFG=${CFG:-$OUT/cfg.json}
[ -f $CFG ] || echo '{"cloth":{"on":false},"people":{"count":0},"features":{"on":true}}' > $CFG
SEEDS=(${=SEEDS:-3 6 8 102 109 110 201 204 205 207})
for s in $SEEDS; do
  t0=$(date +%s)
  env INVARIANTS=1 SERIAL_ROOMS=1 ${=EXTRA} $G $s LivingRoom $OUT/h$s.glb --config $CFG > $OUT/h$s.log 2>&1; rc=$?
  t1=$(date +%s)
  [[ $rc -ne 0 ]] && { echo "seed $s FAILED rc=$rc"; continue }
  echo "seed $s $((t1 - t0))s $(grep '^INVARIANTS' $OUT/h$s.log | cut -d' ' -f2-)"
  rm -f $OUT/h$s.glb $OUT/h$s.usdc
done | tee $OUT/summary.txt
python3 - $OUT/summary.txt <<'PY'
import sys, collections
tot = collections.Counter(); n = 0
for l in open(sys.argv[1]):
    p = l.split()
    if len(p) < 4 or p[0] != 'seed': continue
    n += 1; tot['total'] += int(p[3])
    for kv in p[4:]:
        k, v = kv.split('='); tot[k] += int(v)
print(f"{n} seeds:", ' '.join(f"{k}={v}" for k, v in sorted(tot.items())))
PY
