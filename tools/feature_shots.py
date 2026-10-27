#!/usr/bin/env python3
"""feature_shots.py X.glb OUT_PREFIX [--dist 3.2]: one preview.sh render per room feature (from the sidecar objects[].feature), camera in front of the feature
looking at its centre; prints the written png paths. Needs tools/preview.sh (real-time previewer), no Blender."""
import json, math, subprocess, sys, os
glb, out = sys.argv[1], sys.argv[2]
dist = float(sys.argv[sys.argv.index('--dist') + 1]) if '--dist' in sys.argv else 3.2
only = sys.argv[sys.argv.index('--class') + 1] if '--class' in sys.argv else None
d = json.load(open(glb[:-4] + '.json'))
here = os.path.dirname(os.path.abspath(__file__))
n = 0
for o in d['objects']:
    f = o.get('feature')
    if not f or (only and only not in o['class']): continue
    ox, oy, oz = f['origin']; yaw = f['yaw']
    xc = (f['x'][0] + f['x'][1]) / 2
    c, s = math.cos(yaw), math.sin(yaw)
    w = lambda x, y: (ox + c * x - s * y, oy + s * x + c * y)
    dd = dist
    tx, ty = w(xc, 0.2); ex, ey = w(xc, dd)
    png = f"{out}_{n}_{o['class']}.png"
    subprocess.run([os.path.join(here, 'preview.sh'), glb, '--at', f"{ex},{ey},{oz+1.4},{tx},{ty},{oz+1.0},24", png], check=True, capture_output=True)
    print(png); n += 1
