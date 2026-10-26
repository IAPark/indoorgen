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
        for i in r.get('issues', []):
            issues[i.split(' ')[0] + ' ' + (i.split(' ')[1] if i.split(' ')[0] in ('through','blocks','outside','resting') else '')].append((j['scene'], r['key'], r['class'], i))
    for s in j.get('door_swing_limited', []): tot['door_swing_limited'] += 1
dens = collections.defaultdict(list)
for f in sorted(glob.glob(os.path.join(d, '*.json'))):
    j = json.load(open(f))
    if '_p0_' not in j['scene']: continue
    for r in j.get('rooms', []):
        dens[r['room'].split('_')[0]].append(r)
print("DENSITY (plain scenes; per room type: rooms, mean area m2, mean furniture >50cm, mean small items, mean floor footprint fraction)")
for k, v in sorted(dens.items()):
    n = len(v); print(f"  {k:12s} n={n:2d} area={sum(x['area'] for x in v)/n:5.1f} furniture={sum(x['furniture'] for x in v)/n:4.1f} small={sum(x['small_items'] for x in v)/n:4.1f} footprint={sum(x['footprint_frac'] for x in v)/n:.2f}")
print(f"TOTAL objects={objs} defects={dict(tot)}")
for k, v in sorted(issues.items()):
    print(f"== {k.strip()} ({len(v)})")
    for s, key, cls, i in v[:int(os.environ.get('QC_MAX', 400))]: print(f"  {s} {key} : {i}")
