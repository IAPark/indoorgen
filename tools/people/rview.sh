#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")/.." && pwd); . "$IG_TOOLS/env.sh"
# rview.sh GLB OUT.png X Y DIST H AZ SEED [extra render args]: render one camera of a Rust glb with Blender 5.2 + cloth sims (this worktree's cloth_jobs.py)
G=$1; O=$2; X=$3; Y=$4; D=$5; H=$6; AZ=$7; S=$8; shift 8
T=${TMPDIR:-/tmp}
python3 $IG_TOOLS/people_cam.py $X $Y $D $H $AZ $T/rv_cam.json
cd $INDOORGEN_REF && RES_PCT=${RES_PCT:-50} INDOORGEN_TOOLS=$IG_TOOLS .venv52/bin/python $IG_TOOLS/render/render_rust.py $G $O --cam $T/rv_cam.json --seed $S --samples ${SAMPLES:-24} "$@" > $T/rv.log 2>&1
grep -E "CLOTHJOB|Traceback|Error|FAILED" $T/rv.log | head -12
