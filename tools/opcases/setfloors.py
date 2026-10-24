"""setfloors.py : after `tools/opsuite.sh`, raise (or set) the required level of every suite.txt line to the level reached (only for lines that ran
and were not UNSUPPORTED).  usage: python3 tools/opcases/setfloors.py [id-substr ...]   (reads $TMPDIR/opsuite.res)"""
import os, sys
res = {}
for l in open(os.path.join(os.environ.get("TMPDIR", "/tmp"), "opsuite.res")):
    f = l.split()
    res[f[0]] = (f[3], f[5])
p = os.path.join(os.path.dirname(os.path.abspath(__file__)), "suite.txt")
only = sys.argv[1:]
out = []
for l in open(p).read().split("\n"):
    if l.startswith("#") or not l.strip():
        out.append(l); continue
    f = l.split("|")[:4]
    lvl, st = res.get(f[0], (None, None))
    if st in ("OK", "MISMATCH") and (not only or any(o in f[0] for o in only)):
        f.append(lvl)
    elif len(l.split("|")) > 4:
        f = l.split("|")
    out.append("|".join(f))
open(p, "w").write("\n".join(out))
