"""usage: cmpmats.py ref.json rust.json [-v]  -> per object exact/near status. Names: python skirtingboard_ceiling/support = rust skirting_ceiling/floor"""
import json, sys, re
a = json.load(open(sys.argv[1])); b = json.load(open(sys.argv[2])); v = "-v" in sys.argv
ren = lambda n: {"skirtingboard_ceiling": "skirting_ceiling", "skirtingboard_support": "skirting_floor"}.get(n, n)
a = {ren(k): x for k, x in a.items()}
ok = 0; tot = 0; rooms = {}
for k in sorted(set(a) | set(b)):
    ma = a.get(k, [None])[0]; mb = b.get(k, [None])[0]
    st = "MISSING-rust" if mb is None else "MISSING-ref" if ma is None else "exact" if ma["sig"] == mb["sig"] else "diff"
    r = k.split(".")[0]; rooms.setdefault(r, {})[k.split(".", 1)[-1] if "." in k else k] = st
    if st == "diff":
        la, lb = ma["lines"], mb["lines"]
        same = len(set(la) & set(lb)); st += f" ({ma['name']} vs {mb['name']}; common lines {same}/{max(len(la), len(lb))})"
        if v: print("  ", k, "\n    ref:", "\n    ref: ".join(sorted(set(la) - set(lb))[:6]), "\n    rust:", "\n    rust: ".join(sorted(set(lb) - set(la))[:6]))
    print(f"{k:30s} {st}")
