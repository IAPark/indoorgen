"""cmpbed.py py_prefix rs_prefix : per-part nn vertex distance python vs rust Bed dumps (pairs: frame mattress sheet cover pillow towel by order)"""
import sys, json, numpy as np
from scipy.spatial import cKDTree
def L(p):
    return np.array(json.load(open(p))["verts"])
py, rs = sys.argv[1], sys.argv[2]
def cat(prefix, ids): return np.concatenate([L(f"{prefix}.{i}.json") for i in ids])
pairs = eval(sys.argv[3])  # list of (name, [py ids], [rs ids])
for name, pi, ri in pairs:
    a = cat(py, pi); b = cat(rs, ri)
    d1 = cKDTree(b).query(a)[0]; d2 = cKDTree(a).query(b)[0]
    print("%-8s nv %6d %6d  mean %.2e p95 %.2e max %.2e | bbox py %s..%s rs %s..%s" % (name, len(a), len(b), max(d1.mean(), d2.mean()), max(np.percentile(d1, 95), np.percentile(d2, 95)), max(d1.max(), d2.max()), a.min(0).round(2), a.max(0).round(2), b.min(0).round(2), b.max(0).round(2)))
