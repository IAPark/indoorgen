#!/usr/bin/env python3
"""people_report.py DIR : summarise the "people" section of the qc json files (gen2 --qc) in DIR."""
import json, glob, os, sys, collections
d = sys.argv[1]
kinds = collections.Counter(); cls = collections.Counter(); n = 0; lines = []
per_room = collections.defaultdict(collections.Counter)
for f in sorted(glob.glob(os.path.join(d, '*.json'))):
    j = json.load(open(f)); p = j.get('people')
    if not p: continue
    n += p['n']
    for k, v in p['kinds'].items(): kinds[k] += v
    for room, ks in p['rooms'].items():
        for k in ks: per_room[room.split('_')[0]][k] += 1
    for i in p['issues']:
        c = ('blank wall' if 'blank wall' in i else 'overlap people' if ' overlap ' in i and 'person' in i and 'pillow' not in i else 'same pose' if 'same pose' in i else
             'reach nothing' if 'reaches for' in i else 'toilet crouch' if 'toilet' in i else 'too many people' if ' people in a ' in i else 'unsupported sitter' if 'hips not' in i or 'without support' in i else
             'furniture penetration' if 'furniture' in i else 'through wall' if 'through wall' in i else 'pillow floats' if 'floats' in i else 'pillow oversized' if 'oversized' in i else
             'pillow stacked' if 'overlaps' in i else 'pillow sunk' if 'sunk' in i else 'other')
        cls[c] += 1; lines.append((c, os.path.basename(f)[:-5], i))
print(f"PEOPLE placed={n} kinds={dict(kinds)}")
for r, c in sorted(per_room.items()): print(f"  {r:12s} {dict(c)}")
print(f"ISSUES total={sum(cls.values())} {dict(cls)}")
for c, s, i in sorted(lines): print(f"  [{c}] {s}: {i}")
