#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# dd.sh pymod.Class RustClass seed : dump both and compare
RS=$IG_ROOT/target/release/dumpasset
$RS $2 $3 /tmp/f_rs_$2_$3.json | tail -1
cd $INDOORGEN_REF && .venv/bin/python $IG_TOOLS/pydump.py $1 $3 /tmp/f_py_$2_$3.json 2>&1 | grep PYDUMP
.venv/bin/python $IG_TOOLS/cmpdump.py /tmp/f_py_$2_$3.json /tmp/f_rs_$2_$3.json
