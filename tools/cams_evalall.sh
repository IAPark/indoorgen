#!/bin/zsh
# cams_evalall.sh OUTDIR : the evaluation set, plain and people+mess
for sr in "1 Bedroom" "4 Bedroom" "2 Bedroom" "1 LivingRoom" "3 LivingRoom" "4 LivingRoom" "2 LivingRoom" "0 Kitchen" "1 Kitchen" "3 Kitchen" "2 Kitchen" "0 DiningRoom" "0 Bathroom" "4 Bathroom" "2 Bathroom"; do
  tools/cams_eval.sh $1 plain "$sr" | grep -v "^/"
  tools/cams_eval.sh $1 pm "$sr" --people 2 --mess 0.5 | grep -v "^/"
done
echo ALLDONE
