#!/bin/zsh
# seatcmp.sh seed stage(1..4) : compare python seat stage with rust
cd ~/work/infinigen-ref
[ -f /tmp/f_seat$1.json ] || .venv/bin/python ~/work/agents/furniture/tools/pyseatstages.py $1 /tmp/f_seat$1.json 2>&1 | grep -E "Trace|Error"
names=(curve grid solid subsurf bevel)
.venv/bin/python - $1 $2 <<'PY'
import json,sys
st=json.load(open('/tmp/f_seat%s.json'%sys.argv[1])); names=['curve','grid','solid','subsurf','bevel']
d=st[names[int(sys.argv[2])]]; json.dump({"verts":d["verts"],"polys":d["polys"]},open('/tmp/f_seat_py.json','w'))
PY
STAGE=$2 ~/work/agents/furniture/infinigen-rs/target/release/dumppart chairseat $1 /tmp/f_seat_rs.json >/dev/null 2>&1
echo -n "seed $1 stage $2: "; .venv/bin/python ~/work/agents/furniture/tools/partcmp.py /tmp/f_seat_py.json /tmp/f_seat_rs.json 2>&1 | grep -v WARN
