#!/bin/zsh
# variety.sh [RULES.json|-] [OUT] [SEED0] [N]: a contact sheet of living rooms and bedrooms from N fixed houses, and how far they differ, for judging material rules fast.
# The houses (no outdoors) and their views are generated once into ~/work/bench/var/base; after that a rule file only re-runs reshade and draws the saved views.
# Writes OUT/sheet.jpg and prints the variety numbers. After a geometry change, delete the base. STYLE=name forces one style on every house.
SELF=${0:A}; R=${0:A:h:h}
BASE=$HOME/work/bench/var/base; CFG=$HOME/work/bench/var/cfg_noout.json; G=$R/target/fast/gen2; P=$R/tools/preview.sh; RS=$R/target/fast/reshade
if [[ $1 == --gen ]]; then
  s=$2
  MATERIAL_RULES=none SERIAL_ROOMS=1 $G $s LivingRoom $BASE/h$s.glb --config $CFG > $BASE/h$s.log 2>&1 || { echo "generation failed: $s"; exit 1 }
  rm -f "$BASE/h$s.usdc"
  PREVIEW_SAVE_CAMS=$BASE/c$s $P $BASE/h$s.glb --cams 99 --res 480x270 -o $BASE/v$s > /dev/null 2>&1
  rm -rf "$BASE/v$s"
  exit 0
fi
if [[ $1 == --draw ]]; then
  OUT=$2; s=$3; a=()
  for c in $BASE/c$s/living-room*_A.json(N) $BASE/c$s/bedroom*_A.json(N); do a+=(--cam $c); done
  (( ${#a} )) || exit 0
  exec $P $OUT/h$s.glb $a --res 480x270 -o $OUT/v$s > /dev/null 2>&1
fi
RULES=${1:--}; OUT=${2:-$HOME/work/bench/var/out}; S0=${3:-18000}; N=${4:-20}
[[ -f $CFG ]] || echo '{"cloth":{"on":false},"people":{"count":0},"features":{"on":true},"variation":{"lighting":{"lamps":"auto"}},"program":{"outdoor":{"on":false}}}' > $CFG
mkdir -p $BASE $OUT
seeds=($(seq $S0 $((S0 + N - 1))))
miss=(); for s in $seeds; do [[ -f $BASE/h$s.glb && -d $BASE/c$s ]] || miss+=($s); done
if (( ${#miss} )); then print -l $miss | xargs -P4 -n1 $SELF --gen || exit 1; fi
for s in $seeds; do
  ln -f $BASE/h$s.glb $OUT/h$s.glb
  if [[ $RULES == - ]]; then cp $BASE/h$s.json $OUT/h$s.json
  else $RS $BASE/h$s.json $OUT/h$s.json $RULES --seed $s ${STYLE:+--style} $STYLE 2>/dev/null || { echo "reshade failed on $s"; $RS $BASE/h$s.json $OUT/h$s.json $RULES --seed $s ${STYLE:+--style} $STYLE; exit 1 }; fi
done
print -l $seeds | xargs -P4 -n1 $SELF --draw $OUT
python3 $R/tools/variety_sheet.py $OUT $seeds
for s in $seeds; do rm -f "$OUT/h$s.glb"; done
