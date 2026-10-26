#!/bin/zsh
# cams_eval.sh OUTDIR TAG "SEED ROOM" [gen2 args]: gen2 --cams 3 then preview contact sheet OUTDIR/<room>_<seed>_<tag>.png; deletes glb
OUT=${1:A}; TAG=$2; set -- ${=3} "${@:4}"; S=$1; R=$2; shift 2
G=$PWD/target/fast/gen2; mkdir -p $OUT; T=${TMPDIR:-/tmp}/ev; mkdir -p $T
$G $S $R $T/s.glb --cams ${CAMS:-3} "$@" 2>&1 | grep -E "CAMS"
$PWD/tools/preview.sh $T/s.glb --view cam0 --view cam1 --view cam2 --sheet $OUT/${R}_${S}_${TAG}.png 2>&1 | tail -2
python3 -c "import json;print([(c['name'],c['room'],c['subject']) for c in json.load(open('$T/s.json'))['cameras']])"
rm -f $T/s.glb $T/s.json
