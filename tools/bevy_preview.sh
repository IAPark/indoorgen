#!/bin/zsh
# bevy_preview.sh SCENE.glb [SCENE2.glb ...] [cams options] -o OUT.png|OUT_DIR   (headless Bevy PBR preview; same command
# line as tools/preview.sh). Options: --cams N (auto cameras, 2 per room), --cam cam.json, --view cam0|NAME, --at ex,ey,ez,tx,ty,tz[,lens],
#   --person I, --top, --sheet, --res WxH, --ss N (supersampling, default 2), --sun az,el, --exposure E, --ao A (0 = no SSAO),
#   --ambient A, --no-ceiling, --tonemap agx|aces|tony, --saturation S, --fog F, --shadow-dist M, --point-shadows N,
#   --sidecar-lighting, --dump-cams DIR (write the resolved cameras as cam.json files), -v (timings).
# Positional shorthand: bevy_preview.sh X.glb OUT.png   (3 auto cameras -> sheet)
D=${0:A:h}/bevy_preview; B=$D/target/release/bevy_preview
if [[ ! -x $B || -n "$(find $D/src $D/Cargo.toml -newer $B 2>/dev/null)" ]]; then
  (. ~/.cargo/env; cd $D && CARGO_TARGET_DIR=$D/target CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4} cargo build --release 2>&1 | grep -E "^error" -A6) ; fi
args=("$@")
if [[ ${#args} -ge 2 && ! " $* " == *" -o "* ]]; then last=${args[-1]}; args=(${args[1,-2]} -o $last); fi
exec $B "${args[@]}"
