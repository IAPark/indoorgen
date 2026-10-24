"""jcmp.py a.json b.json : counts, same-index max error and nearest-vertex distances (verts both ways) for two {verts,polys} dumps"""
import sys, json, numpy as np
from scipy.spatial import cKDTree
A = json.load(open(sys.argv[1])); B = json.load(open(sys.argv[2]))
va = np.array(A["verts"]); vb = np.array(B["verts"])
print("nv", len(va), len(vb), "nf", len(A["polys"]), len(B["polys"]), end=" | ")
if len(va) == len(vb): print("same-index max %.2e" % np.abs(va - vb).max(), end=" | ")
d1 = cKDTree(vb).query(va)[0]; d2 = cKDTree(va).query(vb)[0]
print("nn mean %.2e max %.2e" % (max(d1.mean(), d2.mean()), max(d1.max(), d2.max())))
