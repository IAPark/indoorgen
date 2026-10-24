"""gn_table.py CASES.json  -> markdown verification table (node family -> passed/failed cases), from `gncheck CASES.json`.
Family = the case name after 'group:' up to the first '.', e.g. 'math:Math.ADD' -> Math."""
import sys, subprocess, collections, os
exe = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "gncheck")
out = subprocess.run([exe, sys.argv[1]], capture_output=True, text=True).stdout
fam = collections.OrderedDict()
for line in out.splitlines():
    if not line.startswith("CASE "):
        continue
    parts = line.split(" ", 3)
    status, name = parts[1], parts[2]
    f = name.split(":", 1)[-1].split(".")[0]
    d = fam.setdefault(f, [0, 0, 0, []])
    if status == "PASS":
        d[0] += 1
        if "SURFACE-EQUAL" in line:
            d[2] += 1
    else:
        d[1] += 1
        d[3].append(name.split(":", 1)[-1] + ": " + (parts[3][:80] if len(parts) > 3 else ""))
print("| node / family | passed | failed | of which surface-equal only | first failure |")
print("|---|---|---|---|---|")
for f, (p, fl, sf, fails) in fam.items():
    print(f"| {f} | {p} | {fl} | {sf} | {fails[0] if fails else ''} |")
tp = sum(v[0] for v in fam.values()); tf = sum(v[1] for v in fam.values())
print(f"| **total** | {tp} | {tf} | | |")
