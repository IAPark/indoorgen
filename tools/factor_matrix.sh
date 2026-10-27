#!/bin/zsh
# factor_matrix.sh [GEN2] [OUTDIR] [seeds...]: independence test of the variation factors. For each house seed: generate the base scene (ONLY_TARGET, Bedroom) and
# one scene per factor change; tools/factor_diff.py must find only the allowed kinds of change (PASS) and every factor must be deterministic (same config twice -> identical glb).
IG_TOOLS=${0:A:h}; B=${1:-$IG_TOOLS/../target/fast/gen2}; O=${2:-$IG_TOOLS/../tmp/fmatrix}; shift 2 2>/dev/null; SEEDS=(${@:-5 7})
mkdir -p $O; export ONLY_TARGET=1; rc=0
ANY='(?!room$).*'
for s in $SEEDS; do
  base=$O/b$s.glb; [ -f $base ] || $B $s Bedroom $base >/dev/null 2>&1
  t() {  # name, "gen2 args", "diff args"
    local n=$1 g=$2 d=$3; local f=$O/${n}_$s.glb
    ${=B} $s Bedroom $f ${=g} >/dev/null 2>$O/${n}_$s.err || { echo "FAIL gen $n seed $s"; rc=1; return; }
    ${=B} $s Bedroom $f.2.glb ${=g} >/dev/null 2>&1; cmp -s $f $f.2.glb && local det=deterministic || { local det=NONDETERMINISTIC; rc=1; }; rm -f $f.2.glb
    local out; out=$(python3 $IG_TOOLS/factor_diff.py $base $f --list 0 ${=d} 2>&1); local r=$?
    local chg=$(echo $out | grep -E '^(recolored|moved|reshaped|added|removed|lights) ' | awk '{printf "%s=%s ", $1, $2}')
    [ $r = 0 ] && echo "PASS seed $s $n [$det] $chg" || { echo "FAIL seed $s $n [$det] $chg"; rc=1; }
  }
  t doors_closed   '--set variation.doors.mode="closed"'  '--allow moved:Door --allow reshaped:Door'
  t doors_open     '--set variation.doors.mode="open"'    '--allow moved:Door --allow reshaped:Door'
  t doors_random   '--set variation.doors.mode="random" --set variation.doors.seed=3' '--allow moved:Door --allow reshaped:Door'
  t walls_hsv      '--set variation.walls.mode="random_hsv"' '--allow recolored:room'
  t walls_seed     '--set variation.walls.mode="random_hsv" --set variation.walls.seed=9' '--allow recolored:room'
  t ceiling_floor  '--set variation.ceiling.mode="random_hsv" --set variation.floor.mode="random_hsv"' '--allow recolored:room'
  t light_night    '--set variation.lighting.mode="fixed" --set variation.lighting.time_of_day_h=23 --set variation.lighting.lamps="on"' '--allow lights:.*'
  t light_off      '--set variation.lighting.lamps="off"' '--allow lights:.*'
  t light_random   '--set variation.lighting.mode="random" --set variation.lighting.lamps="random" --set variation.lighting.seed=4' '--allow lights:.*'
  t furniture_seed '--set variation.furniture.seed=123' '--allow added:.* --allow removed:.* --allow lights:.* --allow cloth:.*'
  t jitter         '--set variation.furniture.jitter.translate_m=0.1 --set variation.furniture.jitter.rotate_deg=8' "--allow moved:$ANY --allow reshaped:$ANY --allow lights:.* --allow cloth:.*"
  t mess           '--set mess.level=0.5' "--allow moved:$ANY --allow reshaped:$ANY --allow added:.* --allow removed:.* --allow lights:.* --allow cloth:.*"
  t mess_seed      '--set mess.level=0.5 --set mess.seed=77' "--allow moved:$ANY --allow reshaped:$ANY --allow added:.* --allow removed:.* --allow lights:.* --allow cloth:.*"
  t people         '--people 2' '--allow added:Person --allow moved:BedFactory --allow reshaped:BedFactory --allow cloth:.*'
  t people_seed    '--people 2 --set people.seed=99' '--allow added:Person --allow moved:BedFactory --allow reshaped:BedFactory --allow cloth:.*'
done
exit $rc
