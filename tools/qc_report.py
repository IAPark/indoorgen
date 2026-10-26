#!/usr/bin/env python3
"""Aggregate the QC json files of tools/qc_place.sh: per-scene counts, totals, and the issue list per class."""
import json, glob, sys, collections, os
d = sys.argv[1]
tot = collections.Counter(); objs = 0
issues = collections.defaultdict(list)
for f in sorted(glob.glob(os.path.join(d, '*.json'))):
    j = json.load(open(f))
    objs += j['objects']
    for k, v in j['counts'].items(): tot[k] += v
    for r in j['results']:
        for i in r['issues']:
            issues[i.split(' ')[0] + ' ' + (i.split(' ')[1] if i.split(' ')[0] in ('through','blocks','outside','resting') else '')].append((j['scene'], r['key'], r['class'], i))
    for s in j.get('door_swing_limited', []): tot['door_swing_limited'] += 1
print(f"TOTAL objects={objs} defects={dict(tot)}")
for k, v in sorted(issues.items()):
    print(f"== {k.strip()} ({len(v)})")
    for s, key, cls, i in v[:int(os.environ.get('QC_MAX', 400))]: print(f"  {s} {key} : {i}")
