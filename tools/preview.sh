#!/bin/zsh
# preview.sh SCENE.glb [SCENE2.glb ...] [cams options] -o OUT.png|OUT_DIR   (real-time wgpu/Metal preview, ~2 s per scene)
# options: --cams N (auto cameras, 2 per room), --cam cam.json (render_rust.py format), --view cam0|NAME (sidecar camera),
#   --at ex,ey,ez,tx,ty,tz[,lens], --person I, --top, --sheet (contact sheet of all cameras/scenes), --res WxH, --sun az,el,
#   --exposure E, --ao A, --no-ceiling, -v (timings). Output: OUT.png = one image (sheet if several), OUT_DIR = one png per camera.
# Positional shorthand: preview.sh X.glb OUT.png   (3 auto cameras -> sheet)
D=${0:A:h}/preview; B=$D/target/release/preview
if [[ ! -x $B || -n "$(find $D/src $D/Cargo.toml -newer $B 2>/dev/null)" ]]; then
  (. ~/.cargo/env; cd $D && CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2} cargo build --release 2>&1 | grep -E "^error" -A6) ; fi
args=("$@")
# shorthand: last arg ends .png/dir without -o
if [[ ${#args} -ge 2 && ! " $* " == *" -o "* ]]; then last=${args[-1]}; args=(${args[1,-2]} -o $last); fi
exec $B "${args[@]}"
