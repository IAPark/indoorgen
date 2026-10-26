#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# f7check.sh: run gen2 on 4 rooms, report user CPU / generate CPU and whether the glb is byte-identical to the recorded baseline (tools/f7_base.md5,
# md5 of the glb before the F7 performance work). The glb is deleted afterwards (they are 100+ MB). Wall time is meaningless on a loaded machine: use user CPU.
cd "$(git rev-parse --show-toplevel)" || exit 1
B=${GEN2:-target/fast/gen2}
for c in "0 Bathroom" "1 Kitchen" "1 Bedroom" "3 LivingRoom"; do set -- ${=c}
  out=${TMPDIR:-/tmp}/f7_$1_$2.glb
  /usr/bin/time -p $B $1 $2 $out 2> ${TMPDIR:-/tmp}/f7.log >/dev/null
  want=$(grep "^$1_$2 " tools/f7_base.md5 | cut -d' ' -f2); got=$(md5 -q $out); rm -f $out
  echo "$2 $1: $([ "$want" = "$got" ] && echo IDENTICAL || echo DIFFERENT) $(grep '^user' ${TMPDIR:-/tmp}/f7.log) $(grep -o 'cpu [0-9.]*' ${TMPDIR:-/tmp}/f7.log | head -1) $(grep '^real' ${TMPDIR:-/tmp}/f7.log)"
done
