#!/bin/zsh
# iter.sh: the outdoor iteration loop (~5 s from an edit to an image).
#   iter.sh house SEED [ROOM]     generate the house once, without the outdoor stage ($IT/house.glb + house.house.json)
#   iter.sh start [view flags]    start the watching Bevy previewer on house.glb + outdoor.glb (e.g. --at ex,ey,ez,tx,ty,tz,lens);
#                                 it re-renders $IT/view.png whenever outdoor.glb is rewritten
#   iter.sh                       rebuild gen2 (incremental), re-run only the outdoor stage, wait for the new view.png; prints timings
#   iter.sh stop                  stop the previewer
# IT (default ~/work/bench/it), CFG (default ~/work/bench/3d/cfg_auto.json).
R=${0:A:h:h}
IT=${IT:-$HOME/work/bench/it}; CFG=${CFG:-$HOME/work/bench/3d/cfg_auto.json}
BP=$R/tools/bevy_preview/target/release/bevy_preview
build() { (cd $R && cargo build -q --profile fast --bin gen2 2>&1 | grep -E '^error' -A8; exit ${pipestatus[1]}) }
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time' }
mkdir -p $IT
case "$1" in
  house)
    build || exit 1
    SERIAL_ROOMS=1 $R/target/fast/gen2 $2 ${3:-LivingRoom} $IT/house.glb --config $CFG --set program.outdoor.on=false --set output.usd=false > $IT/house.log 2>&1 || { tail -5 $IT/house.log; exit 1 }
    echo "house: $IT/house.glb"; exit 0 ;;
  start)
    shift
    [[ -f $IT/outdoor.json ]] || $R/target/fast/gen2 --outdoor-only $IT/house.house.json --config $CFG $IT/outdoor.glb 2>/dev/null
    nohup $BP --watch --base $IT/house.glb $IT/outdoor.glb --ss 1 --res 1280x720 "$@" -o $IT/view.png > $IT/watch.log 2>&1 &
    echo "previewer pid $! (log $IT/watch.log)"; exit 0 ;;
  stop)
    touch $IT/view.stop; exit 0 ;;
esac
t0=$(now)
build || exit 1
t1=$(now)
before=$(stat -f %m $IT/view.png 2>/dev/null || echo 0)
$R/target/fast/gen2 --outdoor-only $IT/house.house.json --config $CFG $IT/outdoor.glb 2>&1 | grep -E "^OUTDOOR (times|exterior)|panicked|error"
t2=$(now)
# wait for the previewer (view.png newer than before; mtime has 1 s resolution, so also watch its log line count)
n0=$(grep -c "^WATCH" $IT/watch.log 2>/dev/null || echo 0)
for i in {1..600}; do
  n=$(grep -c "^WATCH" $IT/watch.log 2>/dev/null || echo 0)
  (( n > n0 )) && break
  sleep 0.02
done
t3=$(now)
printf "build %.1fs  outdoor %.1fs  render %.1fs  total %.1fs  -> %s\n" $((t1 - t0)) $((t2 - t1)) $((t3 - t2)) $((t3 - t0)) $IT/view.png
