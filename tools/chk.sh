#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# chk.sh pymod.Class RustClass seed0 N [inst]  : full topology check
cd $INDOORGEN_REF && .venv/bin/python $IG_TOOLS/pybatch.py $1 $3 $4 /tmp/f_chk_$2.jsonl ${5:-0} 2>&1 | grep -E "Error|error|Traceback" | head -5
$IG_ROOT/target/release/diffasset /tmp/f_chk_$2.jsonl
