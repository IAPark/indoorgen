#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# ragdoll_tilt.sh KINDS [N]: per-person RAGDOLL tilt/pen lines (RAGDOLL_DBG) over a standard scene set, PEOPLE_FORCE=KINDS. glbs are deleted.
cd "$(git rev-parse --show-toplevel)" || exit 1
K=${1:-SitUpright}; N=${2:-3}
for c in "1 Bedroom" "4 Bedroom" "1 LivingRoom" "3 LivingRoom" "4 LivingRoom" "0 DiningRoom" "3 Kitchen"; do set -- ${=c}
  echo "== $K seed=$1 $2"; PEOPLE_FORCE=$K RAGDOLL_DBG=1 target/fast/gen2 $1 $2 ${TMPDIR:-/tmp}/rt.glb --people $N 2>&1 | grep -E "^RAGDOLL [A-Z]|ghosted" | cut -c1-200
done; rm -f ${TMPDIR:-/tmp}/rt.glb ${TMPDIR:-/tmp}/rt.json
