"""cmpop.py rust_op.json py_op.json [prefix] : per-cutter surface compare of union of pieces"""
import sys, json, numpy as np
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
from surflib import *
R = json.load(open(sys.argv[1]))["pieces"]; P = json.load(open(sys.argv[2])); pre = sys.argv[3] if len(sys.argv) > 3 else ""
def union(ms):
    V = []; F = []
    for m in ms:
        o = len(V); V += m["verts"]; F += [[i + o for i in p] for p in m["polys"]]
    return np.array(V), F
def strip(V, F):
    used = sorted(set(i for p in F for i in p)); mp = {o: n for n, o in enumerate(used)}
    return V[used], [[mp[i] for i in p] for p in F]
for key in sorted([k for k in P if not k.endswith('_rot')], key=lambda k: (k.rstrip('0123456789'), int(k.lstrip('abcdefghijklmnopqrstuvwxyz')))):
    if not key.startswith(pre): continue
    rs = [p for p in R if p["name"].split(".")[0] == key]
    if not rs or not P[key]: print(key, "missing", len(rs), len(P[key])); continue
    va, fa = strip(*union(P[key])); vb, fb = strip(*union(rs))
    d1 = dist(probes(va, fa), vb, fb); d2 = dist(probes(vb, fb), va, fa)
    bb = max(abs(va.min(0) - vb.min(0)).max(), abs(va.max(0) - vb.max(0)).max())
    print("%s: py %d rs %d verts | mean %.2e max %.2e bbox %.2e" % (key, len(va), len(vb), (d1.mean() + d2.mean()) / 2, max(d1.max(), d2.max()), bb))
