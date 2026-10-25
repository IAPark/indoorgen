#!/usr/bin/env python3
"""camjson.py x y z lens out.json : camera at (x,y,z) looking along -y (z up) for render_rust.py --cam"""
import json, sys
x, y, z, lens = map(float, sys.argv[1:5])
R = [[-1, 0, 0], [0, 0, 1], [0, 1, 0]]
m = [[R[i][0], R[i][1], R[i][2], [x, y, z][i]] for i in range(3)] + [[0, 0, 0, 1]]
json.dump({"matrix": m, "lens": lens, "sensor_width": 36}, open(sys.argv[5], "w"))
