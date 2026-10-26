#!/bin/bash
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# run phcheck over all oracle files; summary of TAGDIFF per class and non-exact classes. Env PHTAGS/PHTOL/PHV passed through.
cd "$(dirname "$0")/.."
for f in $INDOORGEN_ORACLES/ph_LR3_hs0b.jsonl $INDOORGEN_ORACLES/traces/case_*_ph.jsonl; do echo "== $f"; target/fast/phcheck "$f" 2>&1; done
