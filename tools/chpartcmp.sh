#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# chpartcmp.sh seed partindex(python list index) rustargs... : compare a python chair part (/tmp/f_ch$seed.json) with `dumppart <args>`
cd $INDOORGEN_REF
[ -f /tmp/f_ch$1.json ] || .venv/bin/python $IG_TOOLS/pychair.py seating.chairs.chair.ChairFactory $1 /tmp/f_ch$1.json 2>&1 | grep -E "Trace|Error"
.venv/bin/python - $1 $2 <<'PY'
import json,sys
d=json.load(open('/tmp/f_ch%s.json'%sys.argv[1])); p=d['parts'][int(sys.argv[2])]
json.dump({"verts":p["verts"],"polys":p["polys"]},open('/tmp/f_chp_py.json','w'))
PY
shift 2
$IG_ROOT/target/release/dumppart $@ /tmp/f_chp_rs.json >/dev/null 2>&1
.venv/bin/python $IG_TOOLS/partcmp.py /tmp/f_chp_py.json /tmp/f_chp_rs.json 2>&1 | grep -v WARN
