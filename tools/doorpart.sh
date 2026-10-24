#!/bin/zsh
# doorpart.sh Class seed const_seed part  : compare python door part with rust (door body only for now)
cd ~/work/infinigen-ref
.venv/bin/python ~/work/agents/furniture/tools/pydoor.py $1 $2 $3 /tmp/f_dd_$1_$2.json 2>&1 | grep -E "Trace|rror" | head -3
D=$(.venv/bin/python -c "import json;c=json.load(open('/tmp/f_dd_$1_$2.json'))['consts'];print(c['width'],c['size'],c['wall'])")
PART=$5 ~/work/agents/furniture/infinigen-rs/target/release/dumppart doorfull $1 $2 /tmp/f_dd_rs.json ${=D} >/dev/null
echo -n "$1 $2: "; .venv/bin/python ~/work/agents/furniture/tools/partcmp.py /tmp/f_dd_$1_$2.json:$4 /tmp/f_dd_rs.json 2>&1 | grep -v WARN
