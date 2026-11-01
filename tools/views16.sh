#!/bin/zsh
# views16.sh [DIR]: the 16 eval views (8 rooms x cornerA / cornerB, the set the Blender passes use) picked by the Rust port of
# viewcam.py (tools/bevy_preview/src/viewpick.rs) and rendered with the Bevy previewer as one sheet per house, in seconds.
# DIR (default ~/work/bench/vc16) holds h11 / h58 / k31 / h204 .glb (regenerated when missing). Cameras for Blender (--cam) go
# to DIR/cams/. Prints the picks (VIEWCAM lines) and the time per house.
R=${0:A:h:h}; D=${1:-$HOME/work/bench/vc16}; BP=$R/tools/bevy_preview/target/release/bevy_preview
CFG=${CFG:-$HOME/work/bench/3d/cfg_auto.json}
mkdir -p $D/cams
typeset -A rooms
rooms=(h11 "bedroom_0/0" h58 "bathroom_0/1" k31 "kitchen_0/0" h204 "kitchen_0/0 living-room_0/0 dining-room_0/0 bathroom_0/0 bedroom_0/0")
typeset -A gen
gen=(h11 "11 Bedroom" h58 "58 Bathroom" k31 "31 Kitchen" h204 "204 LivingRoom")
t0=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
for h in h11 h58 k31 h204; do
  [[ -f $D/$h.glb ]] || SERIAL_ROOMS=1 $R/target/fast/gen2 ${=gen[$h]} $D/$h.glb --config $CFG > $D/gen_$h.log 2>&1
  args=()
  for r in ${=rooms[$h]}; do args+=(--corner "${r}:A" --corner "${r}:B"); done
  t=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  $BP $D/$h.glb $args --res 640x360 --ss 1 --sheet --dump-cams $D/cams -o $D/views16_$h.png 2>&1 | grep -E "^VIEWCAM|viewpick"
  printf "%s: %.1fs -> %s\n" $h $(( $(perl -MTime::HiRes=time -e 'printf "%.3f", time') - t )) $D/views16_$h.png
done
printf "total %.1fs\n" $(( $(perl -MTime::HiRes=time -e 'printf "%.3f", time') - t0 ))
