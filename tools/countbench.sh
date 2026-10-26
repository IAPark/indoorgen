#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
export ONLY_TARGET=1  # these gates compare/measure a single room (gen2 default is now the whole house)
# countbench.sh [gen2]: standalone object counts (no oracles) vs python solve_state.json over every reference room.
# Single seeds swing a lot (23..45 for identical inputs); compare MEANS over all rooms, not single cases.
G=${1:-./target/fast/gen2}; cd "$(git rev-parse --show-toplevel)" || exit 1
W=$(mktemp -d /tmp/countbench.XXXXXX); tp=0; tr=0; n=0
for f in $INDOORGEN_ORACLES/*_[0-9]/solve_state.json; do
  d=$(basename $(dirname $f)); r=${d%_*}; s=${d#*_}
  py=$(python3 -c "
import json; d=json.load(open('$f'))['objs']
print(len([k for k in d if not k.startswith(('door','window','entrance','open')) and not k.endswith('/0')]))")
  rs=$($G $s $r $W/c.glb 2>&1 | grep "^objects" | python3 -c "import sys,re;print(sum(int(x) for x in re.findall(r': (\d+)',sys.stdin.read())))")
  echo "$d py $py rust $rs"; tp=$((tp+py)); tr=$((tr+rs)); n=$((n+1))
done
echo "MEAN over $n rooms: python $((tp*10/n))e-1  rust $((tr*10/n))e-1"
rm -rf $W
