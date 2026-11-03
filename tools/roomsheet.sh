#!/bin/zsh
# roomsheet.sh SEED [OUTDIR]: one house, generation plus a Rust preview sheet of every room (the previewer's room cameras), timed end to end.
# Prints the seconds and invariants; the sheet is OUTDIR/SEED.png. OUTDOOR=true generates the outdoors too.
R=${0:A:h:h}; S=$1; D=${2:-$HOME/work/bench/goal}; mkdir -p $D; cd $D
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time' }
t0=$(now)
SERIAL_ROOMS=1 INVARIANTS=1 $R/target/fast/gen2 $S LivingRoom $D/h$S.glb --config ${CFG:-$HOME/work/bench/3d/cfg_auto.json} --set program.outdoor.on=${OUTDOOR:-false} > $D/h$S.log 2>&1
t1=$(now)
$R/tools/preview.sh $D/h$S.glb --cams 24 --cams auto --sheet --res 640x360 -o $D/$S.png > $D/p$S.log 2>&1
t2=$(now)
printf "%s gen %.1f s preview %.1f s total %.1f s | %s\n" $S $((t1-t0)) $((t2-t1)) $((t2-t0)) "$(grep '^INVARIANTS' $D/h$S.log | cut -c1-60)"
