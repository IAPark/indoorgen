#!/bin/zsh
# chk.sh pymod.Class RustClass seed0 N [inst]  : full topology check
cd ~/work/infinigen-ref && .venv/bin/python ~/work/agents/furniture/tools/pybatch.py $1 $3 $4 /tmp/f_chk_$2.jsonl ${5:-0} 2>&1 | grep -E "Error|error|Traceback" | head -5
~/work/agents/furniture/infinigen-rs/target/release/diffasset /tmp/f_chk_$2.jsonl
