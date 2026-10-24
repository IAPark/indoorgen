"""surfcmp.py cls file.jsonl [seed...] : for each entry run dumpasset (env passes through) and compare surfaces both ways
(python verts + 3 points per edge -> nearest rust triangle surface and vice versa). Prints mean/max per seed and summary."""
import sys, json, subprocess, numpy as np, bpy, mathutils
from mathutils.bvhtree import BVHTree
def tris(V, P):
    T = []
    for p in P:
        for k in range(1, len(p) - 1): T.append((p[0], p[k], p[k + 1]))
    return T
def probes(V, P):
    E = set()
    for p in P:
        for i in range(len(p)): a, b = p[i], p[(i + 1) % len(p)]; E.add((min(a, b), max(a, b)))
    E = np.array(sorted(E)).reshape(-1, 2)
    pts = [V]
    for f in (0.25, 0.5, 0.75): pts.append(V[E[:, 0]] + (V[E[:, 1]] - V[E[:, 0]]) * f)
    A = np.concatenate(pts)
    if len(A) > 20000: A = A[::int(np.ceil(len(A) / 20000))]
    return A
def dist(A, V, P):
    bvh = BVHTree.FromPolygons([tuple(v) for v in V], [tuple(int(i) for i in t) for t in tris(V, P)])
    out = np.empty(len(A))
    for i, p in enumerate(A):
        loc = bvh.find_nearest(mathutils.Vector(p))[0]
        out[i] = (loc - mathutils.Vector(p)).length if loc is not None else 9.0
    return out
