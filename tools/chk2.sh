#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# chk2.sh pymod.Class RustClass seed0 N : set-agreement check (PH=1 for placeholder meshes)
cd $INDOORGEN_REF && .venv/bin/python $IG_TOOLS/pybatch.py $1 $3 $4 /tmp/f_chk2_$2.jsonl 0 2>&1 | grep -E "Error|Traceback" | head -3
.venv/bin/python $IG_TOOLS/nncheck.py $2 /tmp/f_chk2_$2.jsonl
