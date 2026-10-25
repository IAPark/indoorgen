#!/usr/bin/env python3
"""specaudit.py [--rooms "Kitchen 1,Bedroom 1,..."] [--gen2 path] [-v]
Runs gen2 on test rooms and prints, per factory class, how many glb nodes (parts) have no material spec
(those render as a flat glb colour). Exit 1 if any class has missing specs not in ALLOW."""
import json, subprocess, sys, os, collections, tempfile, argparse
ROOMS = "Kitchen 1,Bedroom 1,Bedroom 0,Bathroom 0,Kitchen 0,LivingRoom 1,DiningRoom 0,Kitchen 3,Kitchen 2,LivingRoom 2,Bathroom 1,Bedroom 2"
# truly spec-less on purpose (emissive / no shading): class -> reason
ALLOW = {"CeilingLightFactory": "emissive", "PointLampFactory": "emissive"}
ap = argparse.ArgumentParser(); ap.add_argument("--rooms", default=ROOMS); ap.add_argument("--gen2", default="./target/fast/gen2"); ap.add_argument("-v", action="store_true")
a = ap.parse_args()
miss = collections.Counter(); tot = collections.Counter(); names = collections.defaultdict(collections.Counter)
d = tempfile.mkdtemp(prefix="specaudit", dir=os.environ.get("TMPDIR"))
for r in a.rooms.split(","):
    seed, room = r.split()[::-1] if r.split()[0].isdigit() else (r.split()[1], r.split()[0])
    out = f"{d}/x.glb"
    subprocess.run([a.gen2, seed, room, out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    for n in json.load(open(out.replace(".glb", ".json")))["nodes"]:
        c = n["class"]; tot[c] += 1
        if not n.get("spec"):
            miss[c] += 1; names[c][n["material"].split(":")[0]] += 1
bad = 0
for c in sorted(tot):
    if miss[c]:
        why = ALLOW.get(c)
        if not why: bad += miss[c]
        print(f"{c:36s} {miss[c]:4d}/{tot[c]:<4d} {'ALLOW:'+why if why else ''} {dict(names[c]) if a.v or not why else ''}")
print(f"TOTAL missing (excluding allowed): {bad}  / nodes {sum(tot.values())}")
sys.exit(1 if bad else 0)
