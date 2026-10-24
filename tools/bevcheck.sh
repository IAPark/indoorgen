#!/bin/zsh
# bevcheck.sh kind width segs [angle]
cd ~/work/infinigen-ref
.venv/bin/python ~/work/agents/furniture/tools/pybevel.py $1 $2 $3 /tmp/f_bev.json $4 2>&1 | grep -E "PYBEVEL|Error|Trace"
~/work/agents/furniture/infinigen-rs/target/release/bevtest /tmp/f_bev.json $2 $3 /tmp/f_bev_rs.json $4 2>&1 | tail -1
.venv/bin/python - <<'PY'
import json,sys
d=json.load(open('/tmp/f_bev.json')); json.dump(d['py'],open('/tmp/f_bev_py.json','w'))
PY
.venv/bin/python ~/work/agents/furniture/tools/partcmp.py /tmp/f_bev_py.json /tmp/f_bev_rs.json 2>&1 | grep -v WARN
