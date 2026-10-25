#!/usr/bin/env python3
"""ragdoll_plot.py in.json out.png: key (grey) and settled (red) body points from ragdoll_check, side / front / top views with the support boxes."""
import sys, json
import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
d = json.load(open(sys.argv[1]))
fig, ax = plt.subplots(1, 3, figsize=(15, 5))
views = [("side (y,z)", 1, 2), ("front (x,z)", 0, 2), ("top (x,y)", 0, 1)]
for a, (t, i, j) in zip(ax, views):
    for k, c, s in (("key", "#bbbbbb", 1), ("settled", "#d02020", 1)):
        pts = d[k]
        a.scatter([p[i] for p in pts], [p[j] for p in pts], s=s, c=c, label=k)
    for lo, hi in d["boxes"]:
        a.add_patch(plt.Rectangle((lo[i], lo[j]), hi[i] - lo[i], hi[j] - lo[j], fill=False, ec="k"))
    a.set_aspect("equal"); a.set_title(t); a.grid(True, lw=0.3)
ax[0].legend()
plt.tight_layout(); plt.savefig(sys.argv[2], dpi=70)
