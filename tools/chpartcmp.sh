#!/bin/zsh
# chpartcmp.sh seed partindex(python list index) rustargs... : compare a python chair part (/tmp/f_ch$seed.json) with `dumppart <args>`
cd ~/work/infinigen-ref
[ -f /tmp/f_ch$1.json ] || .venv/bin/python ~/work/agents/furniture/tools/pychair.py seating.chairs.chair.ChairFactory $1 /tmp/f_ch$1.json 2>&1 | grep -E "Trace|Error"
.venv/bin/python - $1 $2 <<'PY'
import json,sys
d=json.load(open('/tmp/f_ch%s.json'%sys.argv[1])); p=d['parts'][int(sys.argv[2])]
json.dump({"verts":p["verts"],"polys":p["polys"]},open('/tmp/f_chp_py.json','w'))
PY
shift 2
~/work/agents/furniture/infinigen-rs/target/release/dumppart $@ /tmp/f_chp_rs.json >/dev/null 2>&1
.venv/bin/python ~/work/agents/furniture/tools/partcmp.py /tmp/f_chp_py.json /tmp/f_chp_rs.json 2>&1 | grep -v WARN
