"""img_stats.py RUN_DIR...: Hypersim-style visible objects / classes per image of make_dataset runs (instance maps). Multi-object assets are split: books = parts / 2 (cover + pages), shelf trinket sets and fruit bowls = their parts, bed linen (comforter, throw, each pillow: BedFactory parts >= 4) separate from the
bed (frame + mattress), as Hypersim's instance maps label pillows and blankets on their own"""
import os, sys, json, re, math
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
            ids = np.unique(a)
            whole, parts, cls = set(), {}, set()
            for i in ids:
                v = inst.get(str(int(i)))
                if not v: continue
                cl = v.get("cls") or ""
                if not cl or any(x in cl for x in NOT_OBJ): continue
                cls.add(cl); base = re.sub(r"#\d+$", "", v["name"])
                pi = int(v["name"].rsplit("#", 1)[1]) if "#" in v["name"] else 0
                if cl == "BedFactory" and pi >= 4: parts.setdefault(("p", base), set()).add(v["name"]); continue  # comforter, throw, each pillow
                if "Book" in cl: parts.setdefault(("b", base), set()).add(v["name"])
                elif any(x in cl for x in ("NatureShelfTrinkets", "FruitContainer")): parts.setdefault(("p", base), set()).add(v["name"])
                else: whole.add(base)
            n = len(whole) + sum(math.ceil(len(s) / 2) if k[0] == "b" else len(s) for k, s in parts.items())
            tot.append((n, len(cls)))
t = np.array(tot)
print(sys.argv[1:], "images", len(t), "objects/image mean %.1f median %.0f" % (t[:, 0].mean(), np.median(t[:, 0])), "classes/image %.1f" % t[:, 1].mean(), ">=21 objects %.0f%%" % (100 * (t[:, 0] >= 21).mean()))
