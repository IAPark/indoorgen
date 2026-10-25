#!/bin/zsh
# refall.sh OUT : run refcmp over every cached class (all cached seeds); one line per case into OUT (for before/after diffs of shared-crate changes)
cd "$(git rev-parse --show-toplevel)" || exit 1
out=${1:-/dev/stdout}; : > $out
for d in ~/work/refcache/*/; do c=$(basename $d)
  ${REFCMP:-./target/fast/refcmp} $c 2>&1 | grep -E "^$c [0-9]+_[0-9]+:" | sed -E 's/ rust [0-9.]+s py [0-9.]+s//; s/\| rust.*//' >> $out
done
