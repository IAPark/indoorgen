#!/bin/zsh
# lay.sh [lay args] : rebuild if needed, solve the fixture houses, draw the sheet. Prints each room's line and the total edit-to-sheet time.
# Defaults: every saved house in ~/work/bench/lay/houses, OUT ~/work/bench/lay/out. e.g. lay.sh --room bedroom --why bed --seed 3
R=${0:A:h:h}; L=$HOME/work/bench/lay; mkdir -p $L/out
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time' }
t0=$(now)
(cd $R && ~/.cargo/bin/cargo build -q --profile fast -p ig_layout 2>&1 | grep -E '^error' -A9; exit ${pipestatus[1]}) || exit 1
t1=$(now)
(cd $R && target/fast/lay "$@" -o $L/out/lay.json ${LAY_HOUSES:-$L/houses/*.house.json})
t2=$(now)
python3 $R/tools/layview.py $L/out/lay.json $L/out/sheet.png --cols ${COLS:-6} --scale ${SCALE:-45}
t3=$(now)
printf "build %.1f s, solve %.2f s, draw %.2f s\n" $((t1-t0)) $((t2-t1)) $((t3-t2))
