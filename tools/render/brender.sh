#!/bin/zsh
# brender.sh [render_rust.py arguments...]: render through the running Blender server (tools/render/blender_server.py), waiting for the
# result; NAME=value pairs before the arguments are passed as the job's environment, e.g.
#   brender.sh NO_USD=1 RES_PCT=50 scene.glb out.png --view outdoor:270:25 --samples 16
# Prints the job's seconds and rc; the render log is $BSRV/done/<id>.log. Start the server once with
#   ~/work/infinigen-ref/.venv52/bin/python tools/render/blender_server.py ~/work/bench/bsrv
Q=${BSRV:-$HOME/work/bench/bsrv}
[[ -f $Q/ready ]] || { echo "blender server not running (no $Q/ready)"; exit 2; }
envj=(); args=()
for a in "$@"; do
  if [[ ${#args} -eq 0 && $a =~ '^[A-Z_][A-Z0-9_]*=' ]]; then envj+=("$a"); else args+=("$a"); fi
done
id=$(date +%s%N)_$$
python3 - "$Q/jobs/$id.json" "${#envj}" "${envj[@]}" "${args[@]}" <<'PY'
import json, sys, os
out, n = sys.argv[1], int(sys.argv[2])
env = dict(e.split("=", 1) for e in sys.argv[3:3 + n])
argv = [os.path.abspath(a) if (a.endswith(".glb") or a.endswith(".png") or a.endswith(".json")) else a for a in sys.argv[3 + n:]]
tmp = out + ".tmp"
json.dump({"argv": argv, "env": env}, open(tmp, "w")); os.replace(tmp, out)
PY
while [[ ! -f $Q/done/$id.json ]]; do sleep 0.2; done
cat $Q/done/$id.json; echo
python3 -c "import json,sys; sys.exit(json.load(open('$Q/done/$id.json'))['rc'])"
