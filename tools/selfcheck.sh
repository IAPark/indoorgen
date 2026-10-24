#!/bin/zsh
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
for c in "Kitchen 1" "Bedroom 1" "Bedroom 0" "Bathroom 0" "Kitchen 0" "LivingRoom 1"; do echo "$c (oracles: placeholders only): $(GEN2=$PWD/target/fast/gen2 tools/lockstep.sh ${=c} ph,emu 2>&1 | tail -1)"; done
echo "DiningRoom 0 (all oracles): $(GEN2=$PWD/target/fast/gen2 tools/lockstep.sh DiningRoom 0 state0,ph,ctor 2>&1 | tail -1)"
$B/phcheck ~/work/ref/ph_LR3_hs0b.jsonl 2>&1 | awk '$NF!=0 && NF>=5 {bad++} END {print "phcheck classes with failures:", bad+0}'
rm -rf $SCW
