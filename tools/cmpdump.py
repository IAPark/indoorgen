"""cmpdump.py py.json rs.json : bbox + vertex set nearest distances (both ways) + face count"""
import sys, json, numpy as np
from scipy.spatial import cKDTree
A = json.load(open(sys.argv[1])); B = json.load(open(sys.argv[2]))
va = np.array(A["verts"]); vb = np.array(B["verts"])
print("nverts py %d rs %d  nfaces py %d rs %d" % (len(va), len(vb), len(A["polys"]), len(B["polys"])))
print("py lo", va.min(0), "hi", va.max(0)); print("rs lo", vb.min(0), "hi", vb.max(0))
print("bbox maxdiff %.3g" % max(abs(va.min(0)-vb.min(0)).max(), abs(va.max(0)-vb.max(0)).max()))
d1 = cKDTree(vb).query(va)[0]; d2 = cKDTree(va).query(vb)[0]
print("vert nn: py->rs max %.3g mean %.3g ; rs->py max %.3g mean %.3g" % (d1.max(), d1.mean(), d2.max(), d2.mean()))

if len(va)==len(vb):
    d=np.abs(va-vb).max(1); print("index-aligned: max %.3g mean %.3g argmax %d" % (d.max(), d.mean(), d.argmax()))
    bad=np.where(d>1e-5)[0]; print("n bad(>1e-5):", len(bad), bad[:10])
pa, pb = A["polys"], B["polys"]
if len(pa) == len(pb):
    same = sum(1 for x, y in zip(pa, pb) if x == y)
    def canon(p):
        k = p.index(min(p)); return p[k:] + p[:k]
    cyc = sum(1 for x, y in zip(pa, pb) if canon(x) == canon(y))
    print("polys: identical %d/%d, identical up to rotation %d/%d" % (same, len(pa), cyc, len(pa)))
    badp = [i for i,(x,y) in enumerate(zip(pa,pb)) if canon(x)!=canon(y)]
    print("first differing polys", badp[:5])
else: print("poly count differs")
