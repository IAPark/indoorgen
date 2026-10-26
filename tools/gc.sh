#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# gc.sh pymodule.Class RustClass [N]  -> prints per-seed + summary geometry error
M=$1; C=$2; N=${3:-6}
$IG_ROOT/target/release/showcase $C $N /tmp/fgc_$C.glb 1 >/dev/null 2>&1
cd $INDOORGEN_REF && .venv/bin/python $IG_TOOLS/oracle/geomcmp.py $M $N 1 /tmp/fgc_$C.glb 2>&1 | grep -E "GEOMCMP|GEOMSUM|PYERR|Trace" | sed 's/GEOMCMP //' | cut -c1-120
