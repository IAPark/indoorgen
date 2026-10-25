#!/usr/bin/env python3
"""people_cam.py X Y [DIST] [HEIGHT] [AZ_DEG] out.json: camera json for render_rust.py --cam, looking at (X,Y,0.9) from DIST m away at azimuth AZ."""
import sys, json, math
x, y = float(sys.argv[1]), float(sys.argv[2])
dist = float(sys.argv[3]) if len(sys.argv) > 3 else 3.0
h = float(sys.argv[4]) if len(sys.argv) > 4 else 2.0
az = math.radians(float(sys.argv[5])) if len(sys.argv) > 5 else 0.0
out = sys.argv[6] if len(sys.argv) > 6 else "/tmp/cam.json"
cx, cy, cz = x + dist * math.cos(az), y + dist * math.sin(az), h
tx, ty, tz = x, y, 0.8
f = [tx - cx, ty - cy, tz - cz]; n = math.sqrt(sum(v * v for v in f)); f = [v / n for v in f]
up = [0, 0, 1]
r = [f[1] * up[2] - f[2] * up[1], f[2] * up[0] - f[0] * up[2], f[0] * up[1] - f[1] * up[0]]; n = math.hypot(*r[:2]); r = [v / n for v in r]
u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]]
# blender camera looks down -Z with +Y up: columns = right, up, -forward
m = [[r[0], u[0], -f[0], cx], [r[1], u[1], -f[1], cy], [r[2], u[2], -f[2], cz], [0, 0, 0, 1]]
json.dump({"matrix": m, "lens": 22}, open(out, "w"))
