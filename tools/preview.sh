#!/bin/zsh
# preview.sh SCENE.glb [SCENE2.glb ...] [cams options] -o OUT.png|OUT_DIR   (real-time wgpu/Metal preview, ~2 s per scene)
# options: --cams N (auto cameras, 2 per room), --cam cam.json (render_rust.py format), --view cam0|NAME (sidecar camera),
#   --at ex,ey,ez,tx,ty,tz[,lens], --person I, --top, --sheet (contact sheet of all cameras/scenes), --res WxH, --sun az,el,
#   --rt fast|full (path traced to look like Blender: about 0.4-0.8 s and 0.8-1.8 s a frame at 960x540 once loaded; --seed N: Blender's sky for that --seed),
#   --rt video (frames of a moving camera: give --cam once per frame, -o DIR gets one png per frame. The first frame is a fast picture, the rest build on it from the light cache and history, about 40-70 ms each at 960x540 once loaded. Encode with ffmpeg -framerate 30 -pattern_type glob -i 'DIR/*.png' -c:v libx264 -pix_fmt yuv420p out.mp4),
#   --exposure E, --ao A, --no-ceiling, -v (timings). Output: OUT.png = one image (sheet if several), OUT_DIR = one png per camera.
# Positional shorthand: preview.sh X.glb OUT.png   (3 auto cameras -> sheet)
D=${0:A:h}/preview; B=$D/target/release/preview
if [[ ! -x $B || -n "$(find $D/src $D/Cargo.toml -newer $B 2>/dev/null)" ]]; then
  (. ~/.cargo/env; cd $D && CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2} cargo build --release 2>&1 | grep -E "^error" -A6) ; fi
args=("$@")
# shorthand: last arg ends .png/dir without -o
if [[ ${#args} -ge 2 && ! " $* " == *" -o "* ]]; then last=${args[-1]}; args=(${args[1,-2]} -o $last); fi
exec $B "${args[@]}"
