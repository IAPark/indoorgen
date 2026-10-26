#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# people_report.sh [N...]: placement report (PEOPLE_SUMMARY lines) for the standard scene set
cd "$(git rev-parse --show-toplevel)" || exit 1
Ns=(${@:-1 2 3})
for n in $Ns; do for c in "1 Bedroom" "4 Bedroom" "1 LivingRoom" "3 LivingRoom" "4 LivingRoom" "0 Kitchen" "3 Kitchen" "0 DiningRoom" "0 Bathroom"; do set -- ${=c}
  echo "== N=$n seed=$1 $2"; PEOPLE_REPORT=1 target/fast/gen2 $1 $2 ${TMPDIR:-/tmp}/pr.glb --people $n 2>&1 | grep -E "^PEOPLE"
done; done; rm -f ${TMPDIR:-/tmp}/pr.glb
