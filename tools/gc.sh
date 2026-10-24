#!/bin/zsh
# gc.sh pymodule.Class RustClass [N]  -> prints per-seed + summary geometry error
M=$1; C=$2; N=${3:-6}
~/work/agents/furniture/infinigen-rs/target/release/showcase $C $N /tmp/fgc_$C.glb 1 >/dev/null 2>&1
cd ~/work/infinigen-ref && .venv/bin/python ~/work/ref/scripts/geomcmp.py $M $N 1 /tmp/fgc_$C.glb 2>&1 | grep -E "GEOMCMP|GEOMSUM|PYERR|Trace" | sed 's/GEOMCMP //' | cut -c1-120
