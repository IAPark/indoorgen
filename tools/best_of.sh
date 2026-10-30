#!/bin/zsh
# best_of.sh OUT N [K]: houses START..START+N-1 (START=1 by default), each the lowest-penalty (tools/layout_score.py) of K candidate seeds (i, i+100, i+200, ...);
# keeps OUT/l<i>.json/.log of the winner and writes OUT/best_of.jsonl with every candidate's score
set -e
OUT=$1; N=$2; K=${3:-3}
BIN=${BIN:-$HOME/work/infinigen-rs/target/fast/gen2}
TOOLS=${0:A:h}
mkdir -p $OUT/cand && cd $OUT
[ -f cfg.json ] || echo '{"cloth":{"on":false},"people":{"count":0}}' > cfg.json
: > best_of.jsonl
S0=${START:-1}
for i in $(seq $S0 $((S0 + N - 1))); do
  best=""; bp=1e9
  for k in $(seq 0 $((K-1))); do
    s=$((i + 100*k))
    SERIAL_ROOMS=1 GROUPDBG=1 LAYOUT_DUMP=cand/c$s.json $BIN $s LivingRoom /tmp/lr.glb --config cfg.json > cand/c$s.log 2>&1 || { echo "FAIL $s"; continue; }
    p=$(uv run -q --with shapely $TOOLS/layout_score.py cand/c$s.json | tee -a best_of.jsonl | python3 -c 'import sys,json; print(json.load(sys.stdin)["penalty"])')
    if (( p < bp )); then bp=$p; best=$s; fi
  done
  echo "house $i: seed $best penalty $bp"
  cp cand/c$best.json l$i.json; cp cand/c$best.log l$i.log
done
