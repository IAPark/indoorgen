#!/bin/zsh
# t.sh SEED ROOM : wall + cpu seconds of gen2 (target room only), original program vs dense
for m in 1 0; do
 if [[ $m == 1 ]]; then export INFINIGEN_PROGRAM=1; else unset INFINIGEN_PROGRAM; fi
 print -n "orig=$m "; ${0:A:h}/../target/fast/gen2 $1 $2 /tmp/t.glb --no-glb 2>&1 | grep -a "^total" | sed 's/stages.*cpu/cpu/'
done
