#!/bin/bash
# run phcheck over all oracle files; summary of TAGDIFF per class and non-exact classes. Env PHTAGS/PHTOL/PHV passed through.
cd "$(dirname "$0")/.."
for f in ~/work/ref/ph_LR3_hs0b.jsonl ~/work/ref/traces/case_*_ph.jsonl; do echo "== $f"; target/fast/phcheck "$f" 2>&1; done
