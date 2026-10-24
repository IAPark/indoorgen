"""facecmp.py a.json b.json : compare two {"v","p"} meshes as sets of faces (positions rounded to 1e-6), report differing faces"""
import sys, json, numpy as np
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
def faces(d):
    V = np.array(d["v"]); out = []
    for p in d["p"]:
        pts = [tuple(np.round(V[i], 5)) for i in p]
        k = min(range(len(pts)), key=lambda i: pts[i]); pts = pts[k:] + pts[:k]
        out.append(tuple(pts))
    return out
fa, fb = faces(a), faces(b)
sa, sb = set(fa), set(fb)
print("faces", len(fa), len(fb), "only in a:", len(sa - sb), "only in b:", len(sb - sa))
for f in list(sa - sb)[:4]: print("A", f)
for f in list(sb - sa)[:4]: print("B", f)
