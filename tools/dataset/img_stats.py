"""img_stats.py RUN_DIR...: per-image visible object instances / classes (>= 0.1% of pixels) from instance maps, Hypersim-style"""
import os, sys, json, re
import numpy as np
from PIL import Image
NOT_OBJ = ("room", "skirting", "Door", "Window", "Person")
tot = []
for run in sys.argv[1:]:
    for h in sorted(os.listdir(run)):
        b = os.path.join(run, h, "base")
        if not os.path.exists(os.path.join(b, "instances.json")): continue
        inst = json.load(open(os.path.join(b, "instances.json")))
        for f in sorted(os.listdir(os.path.join(b, "instance"))):
            a = np.array(Image.open(os.path.join(b, "instance", f)))
            ids, cnt = np.unique(a, return_counts=True)
            objs, cls = set(), set()
            for i, c in zip(ids, cnt):
                v = inst.get(str(int(i)))
                if not v or c < a.size * float(os.environ.get("THR", "0.001")): continue
                cl = v.get("cls") or ""
                if not cl or any(x in cl for x in NOT_OBJ): continue
                objs.add(re.sub(r"#\d+$", "", v["name"])); cls.add(cl)
            tot.append((len(objs), len(cls)))
t = np.array(tot)
print("images", len(t), "objects/image mean %.1f median %.0f" % (t[:, 0].mean(), np.median(t[:, 0])), "classes/image mean %.1f" % t[:, 1].mean(), ">=21 objects %.0f%%" % (100 * (t[:, 0] >= 21).mean()))
