#!/usr/bin/env python3
"""layvar.py ROOM_SUBSTR [--seeds N] [--houses M] [--rules FILE] [--sheet HOUSE ROOM OUT.png]: how much a room's layout changes across seeds, the plan fixed.
For each item: how far it moves (mean over rooms of the mean pairwise distance, m), how many different walls it faces, how often it appears.
`layout difference` per room: mean distance of matched items between two seeds, an item in one and not the other counting 1 m."""
import json, os, subprocess, sys, glob, math, itertools, collections
a = sys.argv[1:]; room = a[0]
opt = lambda k, d: a[a.index(k) + 1] if k in a else d
N = int(opt("--seeds", 6)); M = int(opt("--houses", 14)); rules = opt("--rules", "rules/layout.json")
R = os.path.dirname(os.path.dirname(os.path.abspath(__file__))); L = os.path.expanduser("~/work/bench/lay"); os.makedirs(L + "/out", exist_ok=True)
houses = sorted(glob.glob(L + "/houses/*.house.json"))[:M]
runs = {}
for s in range(N):
    o = f"{L}/out/var{s}.json"
    subprocess.run([f"{R}/target/fast/lay", "--rules", f"{R}/{rules}", "--room", room, "--seed", str(s), "-o", o] + houses, stdout=subprocess.DEVNULL, check=True, cwd=R)
    runs[s] = json.load(open(o))
keyed = collections.defaultdict(dict)
for s, d in runs.items():
    for e in d: keyed[(e["house"], e["sol"]["room"]["name"])][s] = e["sol"]
def center(p):
    c, sn = math.cos(p["pose"]["yaw"]), math.sin(p["pose"]["yaw"]); x, y = (p["lo"][0] + p["hi"][0]) / 2, (p["lo"][1] + p["hi"][1]) / 2
    return [c * x - sn * y + p["pose"]["t"][0], sn * x + c * y + p["pose"]["t"][1]]
moves = collections.defaultdict(list); walls = collections.defaultdict(list); pres = collections.defaultdict(list); diffs = []; ms = []
for k, by in keyed.items():
    ids = set(p["id"].split("#")[0] for s in by.values() for p in s["placed"]) if False else set(p["id"] for s in by.values() for p in s["placed"])
    for sol in by.values(): ms.append(sol["ms"])
    for i in ids:
        cs = [center(p) for s in by.values() for p in s["placed"] if p["id"] == i]
        ys = [round(math.degrees(p["pose"]["yaw"]) / 10) for s in by.values() for p in s["placed"] if p["id"] == i]
        base = i.split("#")[0]; pres[base].append(len(cs) / len(by))
        if len(cs) >= 2: moves[base].append(sum(math.dist(u, v) for u, v in itertools.combinations(cs, 2)) / (len(cs) * (len(cs) - 1) / 2)); walls[base].append(len(set(ys)))
    sl = list(by.values())
    for u, v in itertools.combinations(sl, 2):
        pu, pv = {p["id"]: center(p) for p in u["placed"]}, {p["id"]: center(p) for p in v["placed"]}
        ks = set(pu) | set(pv)
        if ks: diffs.append(sum(math.dist(pu[x], pv[x]) if x in pu and x in pv else 1.0 for x in ks) / len(ks))
mean = lambda l: sum(l) / max(len(l), 1)
print(f"{room}: {len(keyed)} rooms x {N} seeds; layout difference {mean(diffs):.2f} m; solve mean {mean(ms):.0f} ms")
print(f"{'item':12} {'moves m':>8} {'facings':>8} {'present':>8}")
for b in sorted(pres, key=lambda x: -mean(moves[x])): print(f"{b:12} {mean(moves[b]):8.2f} {mean(walls[b]):8.1f} {mean(pres[b]):8.0%}")
if "--sheet" in a:
    i = a.index("--sheet"); h, rm, out = a[i + 1], a[i + 2], a[i + 3]
    sols = [{"house": f"{h} seed {s}", "sol": by} for s, by in keyed[(h, rm)].items()]
    json.dump(sols, open(L + "/out/varsheet.json", "w"))
    subprocess.run(["python3", f"{R}/tools/layview.py", L + "/out/varsheet.json", out, "--cols", "3", "--scale", "60"], check=True, stdout=subprocess.DEVNULL)
