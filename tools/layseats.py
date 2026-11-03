#!/usr/bin/env python3
"""layseats.py [OUT.json]: seats per room from what `lay` solved (dining chairs at the table; sofas and armchairs in living rooms), how far chairs tuck."""
import json, os, sys, math, collections
d = json.load(open(sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/work/bench/lay/out/lay.json")))
din = collections.Counter(); liv = collections.Counter(); tucks = []; nd = nl = 0; sofa_w = []; noseat = []
for e in d:
    s = e["sol"]; k = s["room"]["kind"]; P = s["placed"]
    if k == "dining-room":
        nd += 1; ch = [p for p in P if p["class"] == "ChairFactory"]; din[len(ch)] += 1
        if not ch: noseat.append(e["house"] + " " + s["room"]["name"])
        t = [p for p in P if p["id"] == "table"]
        if t:
            t = t[0]
            for p in ch:
                # how far the chair's front reaches past the table's nearest edge, along the chair's facing
                c, sn = math.cos(p["pose"]["yaw"]), math.sin(p["pose"]["yaw"])
                f = [p["pose"]["t"][0] + c * p["hi"][0], p["pose"]["t"][1] + sn * p["hi"][0]]
                tc, ts = math.cos(t["pose"]["yaw"]), math.sin(t["pose"]["yaw"])
                dx, dy = f[0] - t["pose"]["t"][0], f[1] - t["pose"]["t"][1]
                lx, ly = tc * dx + ts * dy, -ts * dx + tc * dy
                inx = min(lx - t["lo"][0], t["hi"][0] - lx); iny = min(ly - t["lo"][1], t["hi"][1] - ly)
                tucks.append(min(inx, iny) if inx > -0.5 and iny > -0.5 else -0.5)
    if k == "living-room":
        nl += 1; n = sum(p["class"] in ("SofaFactory", "ArmChairFactory") for p in P); liv[n] += 1
        if n == 0: noseat.append(e["house"] + " " + s["room"]["name"])
        sofa_w += [p["hi"][1] - p["lo"][1] for p in P if p["class"] == "SofaFactory"]
print("dining rooms", nd, "chairs per room", dict(sorted(din.items())))
if tucks: tucks.sort(); print("chair front under the table: median %.2f m, min %.2f, max %.2f (n %d)" % (tucks[len(tucks)//2], tucks[0], tucks[-1], len(tucks)))
print("living rooms", nl, "seats per room", dict(sorted(liv.items())))
if sofa_w: sofa_w.sort(); print("sofa width median %.2f, min %.2f" % (sofa_w[len(sofa_w)//2], sofa_w[0]))
print("no seats:", noseat[:12])
