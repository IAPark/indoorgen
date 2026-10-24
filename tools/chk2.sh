#!/bin/zsh
# chk2.sh pymod.Class RustClass seed0 N : set-agreement check (PH=1 for placeholder meshes)
cd ~/work/infinigen-ref && .venv/bin/python ~/work/agents/furniture/tools/pybatch.py $1 $3 $4 /tmp/f_chk2_$2.jsonl 0 2>&1 | grep -E "Error|Traceback" | head -3
.venv/bin/python ~/work/agents/furniture/tools/nncheck.py $2 /tmp/f_chk2_$2.jsonl
