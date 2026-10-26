#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
cd $INDOORGEN_REF
for s in $@; do
.venv/bin/python $IG_TOOLS/pytowel.py $s /tmp/f_tw$s.json 2>&1 | grep -E "Trace|Error:" | head -2
.venv/bin/python - $s <<'PY' 2>&1 | grep -v WARN
import json,sys
d=json.load(open('/tmp/f_tw%s.json'%sys.argv[1])); s=d['snaps'][-1]; json.dump({"verts":s["verts"],"polys":s["polys"]},open('/tmp/f_tw_py.json','w'))
print('params',{k:v for k,v in d['params'].items() if k in('fold_type','folds')})
PY
$IG_ROOT/target/release/dumppart towel $s /tmp/f_tw_rs.json >/dev/null 2>&1
echo -n "towel $s: "; .venv/bin/python $IG_TOOLS/partcmp.py /tmp/f_tw_py.json /tmp/f_tw_rs.json 2>&1 | grep -v WARN
done
