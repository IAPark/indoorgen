#!/bin/zsh
RS=~/work/agents/furniture/infinigen-rs/target/release/dumppart
for c in "4 2.3 0.45 0.0 0.03 0.1" "4 1.2 1.0 0.02 0.06 0.3" "32 0.6 1.0 0.499 0.04 0.25" "4 0.9 1.0 0.04 0.03 0.15" "32 0.55 1.0 0.499 0.02 0.1"; do
  $RS top ${=c} /tmp/f_t_rs.json >/dev/null
  (cd ~/work/infinigen-ref; .venv/bin/python ~/work/agents/furniture/tools/pytop.py ${=c} /tmp/f_t_py.json >/dev/null 2>&1; echo "case $c"; .venv/bin/python ~/work/agents/furniture/tools/cmpdump.py /tmp/f_t_py.json /tmp/f_t_rs.json | grep -E "bbox|vert nn|bad")
done
