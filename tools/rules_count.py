#!/usr/bin/env python3
"""count placed objects per factory class in a gen2 sidecar json (one object = one '<id>_<Class>' prefix); usage: rules_count.py X.json [Class...]"""
import json, sys, collections, re
d = json.load(open(sys.argv[1]))
c = collections.defaultdict(set)
for n in d['nodes']:
    m = re.match(r'(\d+)_(\w+Factory)#', n['name'])
    if m: c[m.group(2)].add(m.group(1))
want = sys.argv[2:] or sorted(c)
print(' '.join(f'{k}={len(c.get(k, ()))}' for k in want))
