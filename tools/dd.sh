#!/bin/zsh
# dd.sh pymod.Class RustClass seed : dump both and compare
RS=~/work/agents/furniture/infinigen-rs/target/release/dumpasset
$RS $2 $3 /tmp/f_rs_$2_$3.json | tail -1
cd ~/work/infinigen-ref && .venv/bin/python ~/work/agents/furniture/tools/pydump.py $1 $3 /tmp/f_py_$2_$3.json 2>&1 | grep PYDUMP
.venv/bin/python ~/work/agents/furniture/tools/cmpdump.py /tmp/f_py_$2_$3.json /tmp/f_rs_$2_$3.json
