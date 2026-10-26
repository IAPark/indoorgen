#!/bin/zsh
# cams_eval2.sh OUTDIR TAG ROOM SEED... : gen2 --people 2 --mess 0.5 --cams 4 --rust-cloth, preview sheet OUTDIR/<room>_<seed>_<tag>.png (4 cams, 1280x720), deletes the glb
OUT=${1:A}; TAG=$2; R=$3; shift 3
G=${GEN2:-$PWD/target/fast/gen2}; mkdir -p $OUT; T=${TMPDIR:-/tmp}/ev; mkdir -p $T
for S in "$@"; do
  CAMDBG=1 $G $S $R $T/s.glb --rust-cloth --people 2 --mess 0.5 --cams 4 2>&1 | grep -E "CAMS|CAMDBG" | sed "s/^/$R$S /"
  $PWD/tools/preview.sh $T/s.glb --view cam0 --view cam1 --view cam2 --view cam3 --res 1280x720 --sheet -o $OUT/${R}_${S}_${TAG}.png 2>&1 | tail -1
  rm -f $T/s.glb $T/s.json
done
