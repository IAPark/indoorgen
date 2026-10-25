#!/bin/zsh
# cams_batch.sh OUTDIR "SEED ROOM" [extra gen2 args...] : gen2 --cams 3, render every camera (RES_PCT=40, 12 samples), delete the glb.
# files: OUTDIR/<room>_<seed>_<tag>_camN.png (tag = $TAG, default plain). One Blender at a time.
OUT=${1:A}; set -- ${=2} "${@:3}"; S=$1; R=$2; shift 2
G=$PWD/target/fast/gen2; mkdir -p $OUT; T=${TMPDIR:-/tmp}/cb_$R$S; mkdir -p $T
$G $S $R $T/s.glb --cams ${CAMS:-3} "$@" 2>&1 | grep -E "CAMS" 
n=$(python3 -c "import json;print(len(json.load(open('$T/s.json')).get('cameras',[])))")
for i in $(seq 0 $((n-1))); do
  rm_=$(python3 -c "import json;print(json.load(open('$T/s.json'))['cameras'][$i]['room'].split('/')[0])")
  (cd ~/work/infinigen-ref && RES_PCT=40 .venv52/bin/python ~/work/ref/scripts/render_rust.py $T/s.glb $OUT/${R}_${S}_${TAG:-plain}_cam$i.png --view cam$i --room $rm_ --seed $S --samples 12 > $T/r$i.log 2>&1; echo "render cam$i rc=$?")
done
cp $T/s.json $OUT/${R}_${S}_${TAG:-plain}.json
rm -rf $T
