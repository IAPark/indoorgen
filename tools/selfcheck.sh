#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
export INFINIGEN_PROGRAM=1  # python-exact program + steps 100/40/5: the lockstep/placeholder gates compare with python oracles (gen2 default is the dense program)
export ONLY_TARGET=1  # these gates compare/measure a single room (gen2 default is now the whole house)
# selfcheck.sh: post-merge gate: build, determinism (+ tilt check), placeholders.
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
det() {  # determinism (two runs, byte-identical glb) + tilt check on the first run. MODE dense = the shipped default program
  local d=$SCW/det_$1_$2_$3; mkdir -p $d
  if [ "$1" = dense ]; then unset INFINIGEN_PROGRAM; fi
  TILTCHECK=1 $B/gen2 $2 $3 $d/a.glb >/dev/null 2> $d/err || { echo "FAIL gen2 $2 $3"; return; }
  $B/gen2 $2 $3 $d/b.glb >/dev/null 2>&1
  local tag=""; [ "$1" = dense ] && tag=" (dense default)"
  cmp -s $d/a.glb $d/b.glb && echo "deterministic$tag $2 $3" || echo "NONDETERMINISTIC$tag $2 $3"
  local nt=$(grep '^TILTCHECK' $d/err | awk '{print $2}')
  [ "$nt" = 0 ] && echo "no tilted objects: $2 $3" || { echo "FAIL tilted objects ($nt): $2 $3"; grep '^TILTED' $d/err | cut -c1-120; }
  rm -rf $d
}
phc() {
  $B/phcheck $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl 2>&1 | awk '$NF!=0 && NF>=5 {bad++} END {print "phcheck classes with failures:", bad+0}'
}
tags() {  # python face tags (back/front/top/bottom) must equal the Rust placeholder's for every oracle placeholder
  local PHD=$SCW/ph; mkdir -p $PHD; local i=0
  for f in $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl $INDOORGEN_ORACLES/traces/case_*_ph.jsonl; do i=$((i+1)); (PHTAGS=1 $B/phcheck $f > /dev/null 2> $PHD/$i.err) & done; wait
  # BookColumnFactory: its placeholder is the exact row bounds on purpose (python's box assumed every book leaned, 1.3-4x too wide)
  local nt=$(cat $PHD/*.err | grep '^TAGDIFF' | grep -vc 'BookColumnFactory'); [ "$nt" = 0 ] && echo "placeholder tags: OK (0 TAGDIFF)" || { echo "FAIL placeholder tags: $nt TAGDIFF"; cat $PHD/*.err | grep '^TAGDIFF' | grep -v BookColumnFactory | awk '{print $2}' | sort | uniq -c; }
}
# lockstep placement vs python traces (tools/lockstep.sh) is no longer part of the gate: python parity is not a goal (run it by hand if needed)
for c in "5 Bedroom" "6 Kitchen" "3 LivingRoom"; do job det dense ${=c}; done
job det py 1 Kitchen
job phc; job tags
wait
for f in $OUT/*.txt; do cat $f; done
rm -rf $SCW
