"""edgediff.py a.json b.json : number of edges (keyed by rounded endpoint positions) present in one dump but not the other"""
import sys, json, numpy as np
def E(path):
    d = json.load(open(path)); v = np.round(np.array(d["verts"]), 5); s = set()
    for p in d["polys"]:
        for i in range(len(p)):
            x, y = tuple(v[p[i]]), tuple(v[p[(i + 1) % len(p)]]); s.add((min(x, y), max(x, y)))
    return s
a, b = E(sys.argv[1]), E(sys.argv[2]); print(len(a), len(b), "only a", len(a - b), "only b", len(b - a))
