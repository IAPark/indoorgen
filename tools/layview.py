#!/usr/bin/env python3
"""layview.py OUT.json SHEET.png [--cols N] [--scale PX_PER_M]: top-down sheet of what `lay` solved.
Walk: green reached from the doors, amber walkable but cut off. Blue walls with their faces, red door clearances, cyan windows.
Pieces: dark hard, orange door/drawer swings, pale green soft; label = id(variant). --why candidates: green tried and fine, red refused with a letter
(O outside, H hard, D door, P opening, W window, K walk, R reach, X exact). A red title: a must is missing."""
import json, sys
from PIL import Image, ImageDraw
args = sys.argv[1:]; src, dst = args[0], args[1]
cols = int(args[args.index("--cols") + 1]) if "--cols" in args else 4
S = float(args[args.index("--scale") + 1]) if "--scale" in args else 70
sols = json.load(open(src))
LET = {"Outside": "O", "Hard": "H", "Door": "D", "Opening": "P", "Window": "W", "Walk": "K", "Reach": "R", "Exact": "X"}
def draw_room(e):
    s = e["sol"]; r = s["room"]; ol = r["outline"]
    x0, y0 = min(p[0] for p in ol) - 0.3, min(p[1] for p in ol) - 0.3
    x1, y1 = max(p[0] for p in ol) + 0.3, max(p[1] for p in ol) + 0.3
    W, H = int((x1 - x0) * S), int((y1 - y0) * S) + 34
    im = Image.new("RGB", (W, H), (250, 250, 248)); d = ImageDraw.Draw(im, "RGBA")
    P = lambda q: ((q[0] - x0) * S, H - (q[1] - y0) * S)
    for j in range(s["ny"]):
        for i in range(s["nx"]):
            v = s["walk"][j * s["nx"] + i]
            if v:
                c = (150, 225, 160, 255) if v == 2 else (250, 205, 110, 255)
                a = P((s["ox"] + i * .05, s["oy"] + j * .05)); b = P((s["ox"] + (i + 1) * .05, s["oy"] + (j + 1) * .05))
                d.rectangle([a[0], b[1], b[0], a[1]], fill=c)
    d.polygon([P(p) for p in ol], outline=(30, 60, 170))
    for w in r["walls"]: d.line([P(w["a"]), P(w["b"])], fill=(30, 60, 170), width=2)
    for o in r["doors"]:
        a = [o["at"][0] - o["u"][0] * o["hw"], o["at"][1] - o["u"][1] * o["hw"]]; b = [o["at"][0] + o["u"][0] * o["hw"], o["at"][1] + o["u"][1] * o["hw"]]
        dep = min(o["hw"] * 2, .9); c = [b[0] + o["n"][0] * dep, b[1] + o["n"][1] * dep]; e2 = [a[0] + o["n"][0] * dep, a[1] + o["n"][1] * dep]
        d.polygon([P(a), P(b), P(c), P(e2)], fill=(230, 60, 60, 70)); d.line([P(a), P(b)], fill=(200, 30, 30), width=4)
    for o in r["windows"]:
        a = [o["at"][0] - o["u"][0] * o["hw"], o["at"][1] - o["u"][1] * o["hw"]]; b = [o["at"][0] + o["u"][0] * o["hw"], o["at"][1] + o["u"][1] * o["hw"]]
        d.line([P(a), P(b)], fill=(0, 190, 220), width=4)
    fill = {0: (70, 70, 80, 190), 1: (240, 150, 40, 70), 2: (120, 200, 120, 80)}
    for p in s["placed"]:
        for q in p["prisms"]:
            if len(q["hull"]) >= 3: d.polygon([P(v) for v in q["hull"]], fill=fill.get(q["role"], (0, 0, 0, 40)), outline=(20, 20, 20))
        # its back (the frame's -x side) and front
        cx = (p["lo"][0] + p["hi"][0]) / 2; cy = (p["lo"][1] + p["hi"][1]) / 2
        import math
        cs, sn = math.cos(p["pose"]["yaw"]), math.sin(p["pose"]["yaw"])
        W2 = lambda x, y: [cs * x - sn * y + p["pose"]["t"][0], sn * x + cs * y + p["pose"]["t"][1]]
        c = W2(cx, cy); f = W2(p["hi"][0], cy)
        d.line([P(c), P(f)], fill=(255, 255, 255), width=2)
        d.text(P(c), p["id"] + ("" if p["variant"] == "as_is" else "(" + p["variant"][:2] + ")"), fill=(255, 255, 255))
    for c in s["why_item"]:
        q = P(c["at"])
        if c["why"]: d.text(q, LET.get(c["why"], "?"), fill=(200, 0, 0))
        else: d.ellipse([q[0] - 2, q[1] - 2, q[0] + 2, q[1] + 2], fill=(0, 150, 0))
    miss = [x["id"] for x in s["report"] if not x["placed"]]
    must = any(not x["placed"] and x["tier"] == 0 for x in s["report"])
    d.rectangle([0, 0, W, 32], fill=(255, 225, 225) if must or not s["doors_joined"] else (235, 235, 235))
    d.text((3, 2), f'{e["house"]} {r["name"]} {r["area"]:.1f} m2 {s["ms"]:.0f} ms' + ("  DOORS CUT" if not s["doors_joined"] else ""), fill=(0, 0, 0))
    d.text((3, 16), ("missing: " + " ".join(miss)) if miss else "complete", fill=(170, 0, 0) if miss else (0, 110, 0))
    return im
ims = [draw_room(e) for e in sols]
if not ims: sys.exit("nothing solved")
cw = max(i.width for i in ims) + 6; rows = [ims[i:i + cols] for i in range(0, len(ims), cols)]
hs = [max(i.height for i in r) + 6 for r in rows]
sheet = Image.new("RGB", (cw * min(cols, len(ims)), sum(hs)), (120, 120, 120))
y = 0
for r, h in zip(rows, hs):
    for k, i in enumerate(r): sheet.paste(i, (k * cw + 3, y + 3))
    y += h
sheet.save(dst)
print(dst, sheet.size)
