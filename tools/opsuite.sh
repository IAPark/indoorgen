#!/bin/bash
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# opsuite.sh [-f] [-b] [ID-substring ...]   run the op regression suite tools/opcases/suite.txt: Blender (tools/opcheck.py) vs Rust (target/fast/opcheck)
#   Blender results are cached in tools/opcases/<id>.blender.json (+ .key = hash of op/case/params); reruns need no Blender.
#   -f   force re-running Blender for the selected ids      -b  build target/fast/opcheck first (fast profile)      -v  show full comparison per line
#   suite.txt line: id|op|case|params|[required level]  (floor = what is reached today, so the suite stays a regression test)
#   ids: only lines whose id contains one of the given substrings.     Prints one line per case + a per-op summary; exit 1 if any MISMATCH.
#   One Blender process at a time (shared cores).  Add a case: add its json (tools/opcases/gen.py) and a line to suite.txt.
cd "$(dirname "$0")/.." || exit 2
PY=${OPPY:-$PY_REF}
BIN=target/fast/opcheck
force=0; build=0; verbose=0
while [[ "$1" == -* ]]; do case "$1" in -f) force=1;; -b) build=1;; -v) verbose=1;; esac; shift; done
if [ $build = 1 ] || [ ! -x $BIN ]; then . ~/.cargo/env; CARGO_BUILD_JOBS=2 cargo build --profile fast --bin opcheck 2>&1 | grep -E "^error" -A6; fi
D=tools/opcases
fail=0; n=0
printf "%-24s %-15s %-8s %-8s %s\n" ID OP LEVEL REQUIRED STATUS
: > ${TMPDIR:-/tmp}/opsuite.res
while IFS='|' read -r id op case params reqf; do
  [[ -z "$id" || "$id" == \#* ]] && continue
  if [ $# -gt 0 ]; then m=0; for s in "$@"; do [[ "$id" == *$s* ]] && m=1; done; [ $m = 0 ] && continue; fi
  key=$(printf '%s\n%s\n%s' "$op" "$params" "$(cat $D/$case.json)" | cksum | cut -d' ' -f1)
  res=$D/$id.blender.json
  if [ $force = 1 ] || [ ! -f $res ] || [ "$(cat $D/$id.blender.key 2>/dev/null)" != "$key" ]; then
    if ! $PY tools/opcheck.py $op --in $D/$case.json --params "$params" --out $res > ${TMPDIR:-/tmp}/opsuite.py.log 2>&1; then
      printf "%-24s %-15s %-8s %-8s %s\n" $id $op - - "BLENDER-ERROR: $(tail -1 ${TMPDIR:-/tmp}/opsuite.py.log | cut -c1-90)"; rm -f $res; continue
    fi
    echo $key > $D/$id.blender.key
  fi
  out=$($BIN $op --in $D/$case.json --params "$params" --blender $res ${reqf:+--require $reqf} 2>&1); rc=$?
  [ $verbose = 1 ] && echo "$out" | sed 's/^/    /'
  line=$(echo "$out" | grep '^RESULT' | tail -1)
  lvl=$(echo "$line" | sed -n 's/.*level=\([a-z]*\).*/\1/p'); req=$(echo "$line" | sed -n 's/.*required=\([^ ]*\).*/\1/p')
  st=$(echo "$line" | sed -n 's/.*status=\(.*\)/\1/p')
  [ -z "$st" ] && { st="CRASH rc=$rc: $(echo "$out" | tail -1 | cut -c1-80)"; rc=1; }
  printf "%-24s %-15s %-8s %-8s %s\n" $id $op "$lvl" "$req" "$st"
  echo "$id $op $case ${lvl:-none} $req ${st%% *}" >> ${TMPDIR:-/tmp}/opsuite.res
  [ $rc = 1 ] && fail=1; n=$((n+1))
done < $D/suite.txt
echo "--- per op: exact/verts/surface/none/unsupported (verts = vertex order exact, face order differs)"
awk '{k=$2; t[k]++; if($6=="UNSUPPORTED")u[k]++; else if($4=="exact")e[k]++; else if($4=="verts")v[k]++; else if($4=="surface")s[k]++; else z[k]++} END{for(k in t) printf "%-15s %d/%d/%d/%d/%d of %d\n",k,e[k],v[k],s[k],z[k],u[k],t[k]}' ${TMPDIR:-/tmp}/opsuite.res | sort
[ $fail = 1 ] && echo "SUITE: mismatches present" || echo "SUITE: no mismatches ($n cases)"
exit $fail
