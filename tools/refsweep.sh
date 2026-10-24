#!/bin/zsh
# refsweep.sh [out.txt]: refcmp SUMMARY for every cached class (what the scene generator's Factory::asset() really produces)
cd "$(git rev-parse --show-toplevel)" || exit 1
CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2} cargo build --profile fast --bin refcmp 2>&1 | grep -E "^error" -A5 && exit 1
for c in $(ls ~/work/refcache); do ./target/fast/refcmp $c --quiet 2>&1 | grep -E "SUMMARY|NO RUST|too big" | tail -1; done | tee ${1:-/dev/null}
