"""sigcmp.py A B: compare two DUMPSIG dumps of render_rust.py (e.g. glb vs usdc import of the same scene): objects, counts, materials,
smoothing, and per object the max difference of world positions (m), corner normals and uvs. Needs numpy."""
import sys, json, numpy as np
a, b = sys.argv[1], sys.argv[2]
ma, mb = json.load(open(a + ".json")), json.load(open(b + ".json"))
na, nb = np.load(a + ".npz"), np.load(b + ".npz")
print("objects", len(ma), len(mb), "only A", len(set(ma) - set(mb)), "only B", len(set(mb) - set(ma)))
meta_diff = {}
worst = {"pos": (0, ""), "cn": (0, ""), "uv": (0, "")}
shape_bad = 0
for k in sorted(set(ma) & set(mb)):
    for f in ma[k]:
        if ma[k][f] != mb[k].get(f):
            meta_diff.setdefault(f, []).append((k, ma[k][f], mb[k].get(f)))
    for f in ("pos", "cn", "uv"):
        x, y = na[k + "|" + f], nb[k + "|" + f]
        if x.shape != y.shape:
            shape_bad += 1
            continue
        if len(x):
            d = float(np.abs(x - y).max())
            if d > worst[f][0]: worst[f] = (d, k)
print("meta fields differing:", {f: len(v) for f, v in meta_diff.items()})
for f, v in meta_diff.items(): print("  e.g.", f, v[0])
print("array shape mismatches:", shape_bad)
for f, (d, k) in worst.items(): print(f"max |{f} diff| {d:.2e}  ({k})")
