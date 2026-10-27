#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
export INFINIGEN_PROGRAM=1  # python-exact program + steps 100/40/5: the lockstep/placeholder gates compare with python oracles (gen2 default is the dense program)
export ONLY_TARGET=1  # these gates compare/measure a single room (gen2 default is now the whole house)
# selfcheck.sh: post-merge gate: build, determinism, lockstep placement, placeholders, tilt.
# The independent checks run as parallel jobs (SELFCHECK_JOBS, default 4; each gen2 capped at RAYON_NUM_THREADS=2); output lines
# and their order are the same as the serial version.
export PATH=$HOME/.cargo/bin:$PATH
cd "$(git rev-parse --show-toplevel)" || exit 1
cargo build --profile fast --bin gen2 --bin phcheck --bin bl32test 2>&1 | grep -E "^error" -A6 && exit 1
B=$PWD/target/fast
SCW=$(mktemp -d /tmp/selfcheck.XXXXXX)
OUT=$SCW/out; mkdir -p $OUT
NJ=${SELFCHECK_JOBS:-4}
export RAYON_NUM_THREADS=${RAYON_NUM_THREADS:-2}
typeset -i n=0
# run "$@" (a function name + args) in the background, at most NJ at once; its stdout goes to $OUT/<seq>.txt
job() {
  while (( $(jobs -r | wc -l) >= NJ )); do sleep 0.2; done
  n=$((n+1)); local f=$OUT/$(printf %03d $n).txt
  ( "$@" > $f 2>&1 ) &
}
det() {  # determinism: two runs, byte-identical glb (MODE=dense: the shipped default program)
  local d=$SCW/det_$1_$2_$3; mkdir -p $d
  if [ "$1" = dense ]; then unset INFINIGEN_PROGRAM; fi
  $B/gen2 $2 $3 $d/a.glb >/dev/null 2>&1 || { echo "FAIL gen2 $2 $3"; return; }
  $B/gen2 $2 $3 $d/b.glb >/dev/null 2>&1
  local tag=""; [ "$1" = dense ] && tag=" (dense default)"
  cmp -s $d/a.glb $d/b.glb && echo "deterministic$tag $2 $3" || echo "NONDETERMINISTIC$tag $2 $3"
  rm -rf $d
}
lock() {  # lockstep placement vs the python trace (informational)
  echo "$1 $2 (oracles: placeholders only): $(GEN2=$B/gen2 tools/lockstep.sh $1 $2 $3 2>&1 | tail -1)"
}
phc() {
  $B/phcheck $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl 2>&1 | awk '$NF!=0 && NF>=5 {bad++} END {print "phcheck classes with failures:", bad+0}'
}
tags() {  # python face tags (back/front/top/bottom) must equal the Rust placeholder's for every oracle placeholder
  local PHD=$SCW/ph; mkdir -p $PHD; local i=0
  for f in $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl $INDOORGEN_ORACLES/traces/case_*_ph.jsonl; do i=$((i+1)); (PHTAGS=1 $B/phcheck $f > /dev/null 2> $PHD/$i.err) & done; wait
  local nt=$(cat $PHD/*.err | grep -c '^TAGDIFF'); [ "$nt" = 0 ] && echo "placeholder tags: OK (0 TAGDIFF)" || { echo "FAIL placeholder tags: $nt TAGDIFF"; cat $PHD/*.err | grep '^TAGDIFF' | awk '{print $2}' | sort | uniq -c; }
}
tilt() {  # no placed object may be tilted more than 0.05 rad off vertical (python itself leaves <= ~0.02 on a few)
  local d=$SCW/tilt_$1_$2; mkdir -p $d
  TILTCHECK=1 $B/gen2 $1 $2 $d/t.glb > /dev/null 2> $d/err
  local nt=$(grep '^TILTCHECK' $d/err | awk '{print $2}')
  [ "$nt" = 0 ] && echo "no tilted objects: $1 $2" || { echo "FAIL tilted objects ($nt): $1 $2"; grep '^TILTED' $d/err | cut -c1-120; }
  rm -rf $d
}
# slowest first so the pool stays busy
job det dense 5 Bedroom; job det dense 6 Kitchen
for c in "3 LivingRoom" "1 Kitchen" "0 Bathroom"; do job det py ${=c}; done
for c in "Kitchen 1" "Bedroom 1" "Bedroom 0" "Bathroom 0" "Kitchen 0" "LivingRoom 1"; do job lock ${=c} ph,emu; done
job lock DiningRoom 0 ph
job phc; job tags
for c in "3 Kitchen" "1 Kitchen" "0 Kitchen" "3 Bedroom" "1 LivingRoom" "0 DiningRoom"; do job tilt ${=c}; done
wait
# print in the serial script's order: py determinism, dense determinism, lockstep, phcheck, tags, tilt
for f in $OUT/{003,004,005,001,002,006,007,008,009,010,011,012,013,014,015,016,017,018,019,020}.txt; do cat $f; done
rm -rf $SCW
