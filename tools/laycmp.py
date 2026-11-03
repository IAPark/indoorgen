#!/usr/bin/env python3
"""laycmp.py A.json B.json: are two `lay` outputs the same layouts? Every item's id, pose and variant, room by room."""
import json, sys
a, b = json.load(open(sys.argv[1])), json.load(open(sys.argv[2]))
key = lambda e: (e["house"], e["sol"]["room"]["name"])
A, B = {key(e): e["sol"] for e in a}, {key(e): e["sol"] for e in b}
bad = 0
for k in A:
    if k not in B: print("missing", k); bad += 1; continue
    pa = [(p["id"], p["variant"], round(p["pose"]["yaw"], 6), round(p["pose"]["t"][0], 6), round(p["pose"]["t"][1], 6)) for p in A[k]["placed"]]
    pb = [(p["id"], p["variant"], round(p["pose"]["yaw"], 6), round(p["pose"]["t"][0], 6), round(p["pose"]["t"][1], 6)) for p in B[k]["placed"]]
    if pa != pb:
        bad += 1
        if bad <= 3: print("differs", k, [x for x in pa if x not in pb][:2], [x for x in pb if x not in pa][:2])
print(f"{len(A)} rooms, {bad} differ")
