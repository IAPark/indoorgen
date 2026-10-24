"""rotcmp.py a.json b.json : are the faces of two meshes equal as cyclic sequences, and do they have the same starting vertex?"""
import sys, json, numpy as np
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2]))
def faces(d, rot):
    V = np.array(d["v"]); out = []
    for p in d["p"]:
        pts = [tuple(np.round(V[i], 4)) for i in p]
        if rot:
            k = min(range(len(pts)), key=lambda i: pts[i]); pts = pts[k:] + pts[:k]
        out.append(tuple(pts))
    return out
ra, rb = set(faces(a, True)), set(faces(b, True))
fa, fb = set(faces(a, False)), set(faces(b, False))
print("faces", len(a["p"]), len(b["p"]), "cyclic-equal:", len(ra & rb), "exact-start-equal:", len(fa & fb))
