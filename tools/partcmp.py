"""partcmp.py py.json[:part] rs.json : surface compare of two mesh dumps"""
import sys, json, numpy as np
sys.path.insert(0, "/Users/lume/work/agents/furniture/tools")
from surflib import *
a = sys.argv[1]
if ":" in a:
    fn, part = a.split(":"); A = json.load(open(fn))["parts" if part != "final" else "final"]; A = A[part] if part != "final" else A
else: A = json.load(open(a))
B = json.load(open(sys.argv[2]))
def strip(M):
    V = np.array(M["verts"]); used = sorted(set(i for p in M["polys"] for i in p)); mp = {o: n for n, o in enumerate(used)}
    return V[used], [[mp[i] for i in p] for p in M["polys"]]
va, pa = strip(A); vb, pb = strip(B); A = {"polys": pa}; B = {"polys": pb}
d1 = dist(probes(va, A["polys"]), vb, B["polys"]); d2 = dist(probes(vb, B["polys"]), va, A["polys"])
bb = max(abs(va.min(0) - vb.min(0)).max(), abs(va.max(0) - vb.max(0)).max())
print("py %d verts rs %d verts | mean %.2e max %.2e (py->rs mean %.2e max %.2e, rs->py mean %.2e max %.2e) bbox %.2e" % (len(va), len(vb), (d1.mean() + d2.mean()) / 2, max(d1.max(), d2.max()), d1.mean(), d1.max(), d2.mean(), d2.max(), bb))
if len(sys.argv) > 3:
    PA, PB = probes(va, A["polys"]), probes(vb, B["polys"])
    for nm, P, d in (("py->rs", PA, d1), ("rs->py", PB, d2)):
        idx = np.argsort(-d)[:8]
        print(nm, ["(%.4f,%.4f,%.4f)=%.1e" % (P[i][0], P[i][1], P[i][2], d[i]) for i in idx])
