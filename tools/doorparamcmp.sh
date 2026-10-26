#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# doorparamcmp.sh Class seed const_seed : compare python params with rust DoorSpec debug print
cd $INDOORGEN_REF && .venv/bin/python $IG_TOOLS/pydoor.py $1 $2 $3 /tmp/f_dp.json 2>&1 | grep -E "Trace|rror" | head -3
read w s t <<< $(.venv/bin/python -c "import json;c=json.load(open('/tmp/f_dp.json'))['consts'];print(c['width'],c['size'],c['wall'])")
$IG_ROOT/target/release/doorparams $1 $2 $w $s $t | tail -1 > /tmp/f_dp_rs.txt
.venv/bin/python - <<'PY'
import json,re
p=json.load(open('/tmp/f_dp.json'))['params']; t=open('/tmp/f_dp_rs.txt').read()
def get(k):
    m=re.search(r'\b'+k+r': ([^,\]]+|\[[^\]]*\])',t); return m.group(1) if m else None
bad=0
for k,v in p.items():
    if v is None or isinstance(v,(str,)) : continue
    r=get(k)
    if r is None: continue
    try:
        if isinstance(v,list): rv=[float(x) for x in r.strip('[]').split(',')]; ok=all(abs(a-b)<1e-9 for a,b in zip(v,rv))
        elif isinstance(v,bool): ok=(r.strip()==('true' if v else 'false'))
        else: ok=abs(float(r)-v)<1e-9
    except Exception as e: ok=None
    if not ok: bad+=1; print("DIFF",k,v,r)
print("compared; diffs:",bad, "style/handle", p['handle_type'],p['door_frame_style'],p['door_orientation'])
PY
