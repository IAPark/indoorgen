"""jcmpv.py a.json b.json : exact-order vertex diff + nearest-vertex diff of two {"v","p"} meshes"""
import sys, json, numpy as np
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
A = np.array(a["v"]); B = np.array(b["v"])
print("counts", len(A), len(B), "faces", len(a["p"]), len(b["p"]))
if len(A) == len(B):
    d = np.linalg.norm(A - B, axis=1); print("order err max %.3g  >1e-5: %d" % (d.max(), (d > 1e-5).sum()))
d1 = np.array([np.linalg.norm(B - p, axis=1).min() for p in A]); d2 = np.array([np.linalg.norm(A - p, axis=1).min() for p in B])
print("nn py->rs max %.3g (%d>1e-5) rs->py max %.3g (%d>1e-5)" % (d1.max(), (d1 > 1e-5).sum(), d2.max(), (d2 > 1e-5).sum()))
