#!/usr/bin/env python3
"""glb_nodes.py X.glb SUBSTR... : print world AABB (accessor min/max corners, node matrix applied) of nodes whose name contains any SUBSTR."""
import json, struct, sys
b = open(sys.argv[1], 'rb').read()
n = struct.unpack('<I', b[12:16])[0]
j = json.loads(b[20:20 + n])
for nd in j['nodes']:
    nm = nd.get('name', '')
    if 'mesh' not in nd or not any(s in nm for s in sys.argv[2:]): continue
    m = j['meshes'][nd['mesh']]
    lo = [1e9] * 3; hi = [-1e9] * 3
    M = nd.get('matrix', [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1])
    for p in m['primitives']:
        a = j['accessors'][p['attributes']['POSITION']]
        for c in range(8):
            q = [a['max'][k] if c >> k & 1 else a['min'][k] for k in range(3)]
            w = [sum(M[col * 4 + r] * q[col] for col in range(3)) + M[12 + r] for r in range(3)]
            for k in range(3): lo[k] = min(lo[k], w[k]); hi[k] = max(hi[k], w[k])
    print(nm, [round(x, 3) for x in lo], [round(x, 3) for x in hi])
