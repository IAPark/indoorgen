#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")" && pwd); . "$IG_TOOLS/env.sh"
# people_view.sh SEED ROOMTYPE N PERSON_INDEX AZ_DEG out.png [extra gen2 args]: proxy render looking at one placed person (deletes the glb afterwards)
cd "$(git rev-parse --show-toplevel)" || exit 1
T=${TMPDIR:-/tmp}; s=$1; r=$2; n=$3; pi=$4; az=$5; out=$6; shift 6
L=$(PEOPLE_PROXY=1 PEOPLE_REPORT=1 target/fast/gen2 $s $r $T/pv.glb --people $n "$@" 2>&1 | grep -E "^PEOPLE  " | sed -n "$((pi+1))p")
echo "$L"
xy=$(echo "$L" | sed -E 's/.*at=\(([-0-9.]+),([-0-9.]+)\).*/\1 \2/')
python3 tools/people_cam.py ${=xy} 3.2 1.9 $az $T/pv_cam.json
(cd $INDOORGEN_REF && RES_PCT=50 .venv52/bin/python $IG_TOOLS/render/render_rust.py $T/pv.glb $out --cam $T/pv_cam.json --seed $s --samples 16 --no-cloth >/dev/null 2>&1)
rm -f $T/pv.glb $T/pv.json
