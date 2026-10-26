#!/bin/zsh
IG_TOOLS=$(cd "$(dirname "$0")/.." && pwd); . "$IG_TOOLS/env.sh"
# case.sh SEED ROOM KINDS COVER AZ OUT.png [PERSON_INDEX] [DIST] [H]: gen2 with forced pose kinds / cover variant, render the person, delete the glb
cd $IG_ROOT || exit 1
export TMPDIR=$PWD/tmp
s=$1; r=$2; k=$3; c=$4; az=$5; out=$6; pi=${7:-0}; d=${8:-2.2}; h=${9:-1.5}
L=$(PEOPLE_COVER=$c PEOPLE_FORCE=$k PEOPLE_REPORT=1 target/fast/gen2 $s $r $TMPDIR/case.glb ${=RC} --people ${N:-1} 2>&1 | grep -E "^PEOPLE  " | sed -n "$((pi+1))p")
echo "$L" | cut -c1-120
xy=$(echo "$L" | sed -E 's/.*at=\(([-0-9.]+),([-0-9.]+)\).*/\1 \2/')
tools/people/rview.sh $TMPDIR/case.glb $out ${=xy} $d $h $az $s | grep -c CLOTHJOB
rm -f $TMPDIR/case.glb $TMPDIR/case.json
