#!/bin/zsh
# assets.sh CLASSES [N=4] [OUT=~/work/bench/goal/asset/quick] [SEED0=1]: build N of each asset (comma list: Sink,Toilet,Kettle,...) without a house,
# render each from two sides, write OUT/sheet.jpg. Env: CLOSE=1.0 (closer > 1), RES=480x270.
setopt err_exit
R=${0:A:h:h}; C=$1; N=${2:-4}; O=${3:-$HOME/work/bench/goal/asset/quick}; S=${4:-1}
mkdir -p $O
(cd $R; ~/.cargo/bin/cargo build --profile fast --bin assetsheet 2>&1 | grep -E "^error" -A8 || true)
$R/target/fast/assetsheet $C $N $O/a.glb $S
python3 $R/tools/assets_sheet.py $O $R/tools/preview.sh
