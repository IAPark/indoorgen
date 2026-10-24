"""nnhash.py a.json b.json [cell] : vertex nearest-neighbour distances (spatial hash, exact within `cell`), both directions; prints worst vertices"""
import sys, json, numpy as np, collections
a = np.array(json.load(open(sys.argv[1]))['v']); b = np.array(json.load(open(sys.argv[2]))['v'])
cell = float(sys.argv[3]) if len(sys.argv) > 3 else 0.01
def build(P):
    g = collections.defaultdict(list)
    for i, p in enumerate(P): g[tuple((p // cell).astype(int))].append(i)
    return g
def nn(X, Y, gy):
    out = np.full(len(X), np.inf)
    for i, p in enumerate(X):
        k = np.floor(p / cell).astype(int)
        r = 1
        while True:
            best = np.inf
            for dx in range(-r, r + 1):
                for dy in range(-r, r + 1):
                    for dz in range(-r, r + 1):
                        for j in gy.get((k[0] + dx, k[1] + dy, k[2] + dz), ()):
                            best = min(best, np.linalg.norm(Y[j] - p))
            if best <= r * cell or r > 30: break
            r += 1
        out[i] = best
    return out
da = nn(a, b, build(b)); db = nn(b, a, build(a))
print("a->b max %.3g (%d>1e-4 of %d)   b->a max %.3g (%d>1e-4 of %d)" % (da.max(), (da > 1e-4).sum(), len(a), db.max(), (db > 1e-4).sum(), len(b)))
for nm, X, d in (("a", a, da), ("b", b, db)):
    for i in np.argsort(-d)[:5]: print(nm, i, X[i].round(4), "%.3g" % d[i])
for nm, X, d in (("a", a, da), ("b", b, db)):
    m = d > 1e-4
    if m.any(): print(nm, "mismatch bbox", X[m].min(0).round(3), X[m].max(0).round(3), "count", m.sum())
