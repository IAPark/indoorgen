"""plant_tpl.py MODULE.CLASS OUT_PREFIX [--inst N] [--tries 60] : build RNG-replay templates for a node-graph factory.
For every control-flow signature (sequence of numpy draws with their literal arguments) seen in the seeds tried, export the asset
for 4 seeds with that signature and fit every differing constant of the exported graph as a*draw+b of one scalar draw."""
import sys, json, importlib, collections, numpy as np
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
import bpy
import gn_export as GX
from infinigen.core.util.math import int_hash

EV = []
PH = ["ctor"]
def num(x):
    if isinstance(x, (np.floating, float)): return float(x)
    if isinstance(x, (np.integer, int)): return int(x)
    if isinstance(x, np.ndarray): return [num(y) for y in x.ravel().tolist()]
    if isinstance(x, (list, tuple)): return [num(y) for y in x]
    return str(x)
def wrap(name):
    f = getattr(np.random, name)
    def g(*a, **k):
        r = f(*a, **k)
        EV.append({"ph": PH[0], "f": name, "a": num(list(a)), "k": {kk: num(v) for kk, v in k.items() if kk in ("size", "p")}, "r": num(r)})
        return r
    setattr(np.random, name, g)
for n in ("uniform", "normal", "randint", "choice", "rand", "random", "random_sample", "shuffle", "standard_normal", "permutation", "randn", "seed"):
    wrap(n)

def _opt(n, d):
    return int(sys.argv[sys.argv.index(n) + 1]) if n in sys.argv else d
_pos=[]
_skip=False
for _i,_x in enumerate(sys.argv[1:],1):
    if _skip: _skip=False; continue
    if _x.startswith("--"): _skip=True; continue
    _pos.append(_x)
args = _pos
modcls, prefix = args[0], args[1]
inst = int(sys.argv[sys.argv.index("--inst") + 1]) if "--inst" in sys.argv else 0
tries = int(sys.argv[sys.argv.index("--tries") + 1]) if "--tries" in sys.argv else 60
MINS=_opt("--min",4); NU=MINS
mod, cls = modcls.rsplit(".", 1)
C = getattr(importlib.import_module("infinigen.assets.objects." + mod), cls)
GX.stub_materials()

def classify_seed(v, fs):
    if v == fs: return "fs"
    if v == int_hash((fs, inst)): return "inst"
    return "lit"

def run_events(seed):
    EV.clear(); PH[0] = "ctor"
    bpy.ops.wm.read_factory_settings(use_empty=True)
    np.random.seed(seed)
    f = C(seed)
    PH[0] = "asset"
    f.spawn_asset(inst)
    evs = [dict(e) for e in EV]
    for e in evs:
        if e["f"] == "seed": e["cls"] = classify_seed(e["a"][0], seed)
    return evs

def sig(evs):
    return json.dumps([(e["ph"], e["f"], e["a"] if e["f"] != "seed" else e["cls"], e["k"]) for e in evs])

groups = collections.defaultdict(list)
cache = {}
for s in range(1, tries + 1):
    try:
        evs = run_events(s)
    except Exception as ex:
        print("seed", s, "failed", repr(ex)[:100]); continue
    cache[s] = evs
    groups[sig(evs)].append(s)
print("signatures:", sorted([len(v) for v in groups.values()], reverse=True))
print("event counts:", sorted(set(len(cache[v[0]]) for v in groups.values())))

def scalars(evs):
    out = []
    for e in evs:
        if e["f"] == "seed": continue
        r = e["r"]
        if isinstance(r, list): out.extend(x for x in r if isinstance(x, (int, float)))
        elif isinstance(r, (int, float)): out.append(r)
        else: out.append(None)
    return out

def leaves(o, path, acc):
    if isinstance(o, dict):
        for k, v in o.items(): leaves(v, path + [k], acc)
    elif isinstance(o, list):
        for i, v in enumerate(o): leaves(v, path + [i], acc)
    elif isinstance(o, (int, float)) and not isinstance(o, bool):
        acc[tuple(path)] = float(o)

def flat_export(ex):
    acc = {}
    leaves({"groups": ex["groups"], "steps": [{k: v for k, v in s.items() if k not in ("mesh", "expect", "keep")} for s in ex["steps"]]}, [], acc)
    return acc

for gi, (sg, seeds) in enumerate(sorted(groups.items(), key=lambda kv: -len(kv[1]))):
    if len(seeds) < 4:
        continue
    use = seeds[:4]
    exps = []
    for s in use:
        EV.clear(); PH[0] = "asset"
        exps.append(GX.export_asset(modcls, s, inst))
    fl = [flat_export(e) for e in exps]
    keys = set(fl[0])
    for f in fl[1:]:
        keys &= set(f)
    if any(set(f) != set(fl[0]) for f in fl[1:]):
        print("sig", gi, "structure differs between seeds", use); continue
    D = [scalars(cache[s]) for s in use]
    L = min(len(d) for d in D)
    mapping, unm, amb = [], [], 0
    for key in sorted(keys, key=str):
        v = [f[key] for f in fl]
        if max(v) - min(v) <= 1e-7 * max(1.0, abs(v[0])): continue
        found = []
        for k in range(L):
            d = [D[i][k] for i in range(NU)]
            if any(x is None for x in d) or abs(d[0] - d[1]) < 1e-12: continue
            a = (v[0] - v[1]) / (d[0] - d[1]); b = v[0] - a * d[0]
            if all(abs(a * d[i] + b - v[i]) <= 2e-5 * max(1.0, abs(v[i])) for i in range(NU)):
                found.append((k, a, b))
        if not found:
            unm.append({"path": list(key), "v": v})
        else:
            if len(found) > 1: amb += 1
            k, a, b = found[0]
            mapping.append({"path": list(key), "k": k, "a": a, "b": b})
    base = exps[0]
    base.pop("_kwargs", None)
    out = {"base": base, "map": mapping, "unmapped": unm, "program": cache[use[0]], "seed0": use[0], "inst": inst, "sig_seeds": seeds}
    fn = f"{prefix}_{gi}.json"
    json.dump(out, open(fn, "w"), separators=(",", ":"))
    print(f"template {fn}: seeds {seeds[:6]} (n={len(seeds)}) mapped {len(mapping)} unmapped {len(unm)} ambiguous {amb}")
    for u in unm[:12]: print("  unmapped", u["path"][:6], u["path"][-2:], [round(x, 4) for x in u["v"]])
