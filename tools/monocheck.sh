#!/bin/bash
# usage: tools/monocheck.sh SEED [INST]  -- python vs rust monocot final surface (deletes dumps afterwards)
S=$1; I=${2:-0}; cd ~/work/wt/decor; R=tmp/mc_rs_$S; P=tmp/mc_py_$S; mkdir -p $R $P
PYTRACE_RANGE=99999-99999 ~/work/infinigen-ref/.venv/bin/python tools/pytrace.py monocot.MonocotFactory $S $I $P 2>&1 | grep -E "Error|error" | head -3
mv $P/final.bin $P/999_final_out.bin
MONOFINAL=$R target/fast/stagetest monocot $S $I | tail -1
target/fast/stagediff $R $P ${3:-1e-4} | head -3
rm -rf $R $P
