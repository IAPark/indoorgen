#!/bin/bash
# run phcheck over all oracle files; usage: tools/phall.sh  (env PHTAGS/PHTOL passed through)
cd "$(dirname "$0")/.."
for f in ~/work/ref/ph_LR3_hs0b.jsonl ~/work/ref/traces/case_*_ph.jsonl; do target/fast/phcheck "$f"; done
