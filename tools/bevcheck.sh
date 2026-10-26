#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# bevcheck.sh kind width segs [angle]
cd $INDOORGEN_REF
.venv/bin/python $IG_TOOLS/pybevel.py $1 $2 $3 /tmp/f_bev.json $4 2>&1 | grep -E "PYBEVEL|Error|Trace"
$IG_ROOT/target/release/bevtest /tmp/f_bev.json $2 $3 /tmp/f_bev_rs.json $4 2>&1 | tail -1
.venv/bin/python - <<'PY'
import json,sys
d=json.load(open('/tmp/f_bev.json')); json.dump(d['py'],open('/tmp/f_bev_py.json','w'))
PY
.venv/bin/python $IG_TOOLS/partcmp.py /tmp/f_bev_py.json /tmp/f_bev_rs.json 2>&1 | grep -v WARN
