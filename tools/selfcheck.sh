#!/bin/zsh
# selfcheck.sh: post-merge gate (~2 min): build, determinism, lockstep placement, placeholders
export PATH=$HOME/.cargo/bin:$PATH
cd ~/work/infinigen-rs || exit 1
cargo build --profile fast --bin gen2 --bin phcheck --bin bl32test 2>&1 | grep -E "^error" -A6 && exit 1
B=./target/fast
for c in "3 LivingRoom" "1 Kitchen" "0 Bathroom"; do set -- ${=c}
  $B/gen2 $1 $2 /tmp/sc_a.glb >/dev/null 2>&1 || { echo "FAIL gen2 $c"; exit 1; }
  $B/gen2 $1 $2 /tmp/sc_b.glb >/dev/null 2>&1; cmp -s /tmp/sc_a.glb /tmp/sc_b.glb && echo "deterministic $c" || echo "NONDETERMINISTIC $c"
done
for c in "Kitchen 1" "Bedroom 1" "Bedroom 0"; do echo "$c: $(GEN2=$B/gen2 ~/work/ref/cmpcase.sh ${=c} 2>&1 | tail -1)"; done
$B/phcheck ~/work/ref/ph_LR3_hs0b.jsonl 2>&1 | awk '$NF!=0 && NF>=5 {bad++} END {print "phcheck classes with failures:", bad+0}'
rm -f /tmp/sc_a.glb /tmp/sc_b.glb
