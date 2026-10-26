#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# chsurf.sh pymod.Class RustClass seed0 N : surface compare asset (not placeholder) py vs rust for seeds seed0..seed0+N-1
unset PH
cd $INDOORGEN_REF
.venv/bin/python $IG_TOOLS/pybatch.py $1 $3 $4 /tmp/f_cs_$2.jsonl 0 2>&1 | grep -E "Error|Traceback" | head -3
for i in $(seq $3 $(($3+$4-1))); do
  .venv/bin/python - $2 $i <<'PY' 2>&1 | grep -v WARN
import json,sys
cls,seed=sys.argv[1],int(sys.argv[2])
for l in open('/tmp/f_cs_%s.jsonl'%cls):
    r=json.loads(l)
    if r['fseed']==seed:
        json.dump({"verts":r["verts"],"polys":r["polys"]},open('/tmp/f_cs_py.json','w'))
PY
  $IG_ROOT/target/release/dumpasset $2 $i /tmp/f_cs_rs.json >/dev/null 2>&1
  echo -n "$2 $i: "; .venv/bin/python $IG_TOOLS/partcmp.py /tmp/f_cs_py.json /tmp/f_cs_rs.json 2>&1 | grep -v WARN | sed 's/(py.*)//'
done
