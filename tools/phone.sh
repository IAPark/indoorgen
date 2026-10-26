#!/bin/bash
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# phone.sh <case-name|file> [class]: phcheck one oracle (PHTAGS/PHVERT/PHTOL from env), optional class filter on stderr
cd "$(dirname "$0")/.."
f=$1; [ -f "$f" ] || f=$INDOORGEN_ORACLES/traces/case_${1}_ph.jsonl
target/fast/phcheck "$f" 2>&1 | { if [ -n "$2" ]; then grep -E "$2|^class"; else cat; fi; }
