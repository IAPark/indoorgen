#!/bin/zsh
# messcheck.sh [levels...] : clutter audit (gen2 MESSCHECK=1) over the reference rooms; one line per scene+level.
# columns: clutter items, max interpenetration vs furniture / other clutter (m), items hovering > 1 cm, max gap, items outside rooms, clutter time
cd "$(git rev-parse --show-toplevel)" || exit 1
G=${GEN2:-./target/fast/gen2}; L=(${@:-0.5 1}); mkdir -p tmp; W=$(mktemp -d tmp/messcheck.XXXXXX)
for c in "Bedroom 1" "Bedroom 4" "LivingRoom 1" "LivingRoom 3" "Kitchen 0" "Kitchen 3" "Bathroom 0" "DiningRoom 0"; do
  set -- ${=c}
  for l in $L; do
    out=$(MESSCHECK=1 $G $2 $1 $W/o.glb --mess $l 2>&1 | grep -E "^MESS")
    rm -f $W/o.glb $W/o.json
    chk=$(echo "$out" | grep MESSCHECK | sed 's/MESSCHECK //'); sum=$(echo "$out" | grep "^MESS level" | sed 's/.*removed=[0-9]* //')
    echo "$1 $2 level $l: $chk $sum"
  done
done
rm -rf $W
