#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
export INFINIGEN_PROGRAM=1  # python-exact program + steps 100/40/5: the lockstep/placeholder gates compare with python oracles (gen2 default is the dense program)
export ONLY_TARGET=1  # these gates compare/measure a single room (gen2 default is now the whole house)
# selfcheck.sh: post-merge gate (~2 min): build, determinism, lockstep placement, placeholders
export PATH=$HOME/.cargo/bin:$PATH
cd "$(git rev-parse --show-toplevel)" || exit 1
cargo build --profile fast --bin gen2 --bin phcheck --bin bl32test 2>&1 | grep -E "^error" -A6 && exit 1
B=./target/fast
SCW=$(mktemp -d /tmp/selfcheck.XXXXXX)
for c in "3 LivingRoom" "1 Kitchen" "0 Bathroom"; do set -- ${=c}
  $B/gen2 $1 $2 $SCW/a.glb >/dev/null 2>&1 || { echo "FAIL gen2 $c"; exit 1; }
  $B/gen2 $1 $2 $SCW/b.glb >/dev/null 2>&1; cmp -s $SCW/a.glb $SCW/b.glb && echo "deterministic $c" || echo "NONDETERMINISTIC $c"
done
# dense default mode (no INFINIGEN_PROGRAM): determinism of the shipped default
for c in "5 Bedroom" "6 Kitchen"; do set -- ${=c}
  (unset INFINIGEN_PROGRAM; $B/gen2 $1 $2 $SCW/a.glb >/dev/null 2>&1 && $B/gen2 $1 $2 $SCW/b.glb >/dev/null 2>&1; cmp -s $SCW/a.glb $SCW/b.glb && echo "deterministic (dense default) $c" || echo "NONDETERMINISTIC (dense default) $c")
done
for c in "Kitchen 1" "Bedroom 1" "Bedroom 0" "Bathroom 0" "Kitchen 0" "LivingRoom 1"; do echo "$c (oracles: placeholders only): $(GEN2=$PWD/target/fast/gen2 tools/lockstep.sh ${=c} ph,emu 2>&1 | tail -1)"; done
echo "DiningRoom 0 (oracles: placeholders only): $(GEN2=$PWD/target/fast/gen2 tools/lockstep.sh DiningRoom 0 ph 2>&1 | tail -1)"
$B/phcheck $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl 2>&1 | awk '$NF!=0 && NF>=5 {bad++} END {print "phcheck classes with failures:", bad+0}'
# placeholder tags gate: python face tags (back/front/top/bottom) must equal the Rust placeholder's for every oracle placeholder
PHD=$SCW/ph; mkdir -p $PHD; i=0
for f in $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl $INDOORGEN_ORACLES/traces/case_*_ph.jsonl; do i=$((i+1)); (PHTAGS=1 $B/phcheck $f > /dev/null 2> $PHD/$i.err) & done; wait
nt=$(cat $PHD/*.err | grep -c '^TAGDIFF'); [ "$nt" = 0 ] && echo "placeholder tags: OK (0 TAGDIFF)" || { echo "FAIL placeholder tags: $nt TAGDIFF"; cat $PHD/*.err | grep '^TAGDIFF' | awk '{print $2}' | sort | uniq -c; }
# standalone: no placed object may be tilted more than 0.05 rad off vertical (python itself leaves <= ~0.02 on a few)
for c in "3 Kitchen" "1 Kitchen" "0 Kitchen" "3 Bedroom" "1 LivingRoom" "0 DiningRoom"; do set -- ${=c}
  nt=$(TILTCHECK=1 $B/gen2 $1 $2 $SCW/t.glb 2>&1 | grep '^TILTCHECK' | awk '{print $2}')
  [ "$nt" = 0 ] && echo "no tilted objects: $c" || { echo "FAIL tilted objects ($nt): $c"; TILTCHECK=1 $B/gen2 $1 $2 $SCW/t.glb 2>&1 | grep '^TILTED' | cut -c1-120; }
done
rm -rf $SCW
