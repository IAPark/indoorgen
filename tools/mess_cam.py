"""mess_cam.py EYE_X EYE_Y EYE_Z TGT_X TGT_Y TGT_Z out.json [lens]: camera json for render_rust.py --cam (look-at, Z up)"""
import sys, json, numpy as np
a = [float(x) for x in sys.argv[1:7]]
eye, tgt = np.array(a[:3]), np.array(a[3:6])
f = tgt - eye; f /= np.linalg.norm(f)
r = np.cross(f, [0, 0, 1]); r /= np.linalg.norm(r)
u = np.cross(r, f)
M = np.eye(4); M[:3, 0] = r; M[:3, 1] = u; M[:3, 2] = -f; M[:3, 3] = eye
json.dump({"matrix": M.tolist(), "lens": float(sys.argv[8]) if len(sys.argv) > 8 else 24}, open(sys.argv[7], "w"))
