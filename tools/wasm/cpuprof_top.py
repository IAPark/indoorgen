#!/usr/bin/env python3
# python3 tools/wasm/cpuprof_top.py FILE.cpuprofile [N]  -- top self-time functions of a node --cpu-prof profile
import json,sys,collections
p=json.load(open(sys.argv[1])); n=int(sys.argv[2]) if len(sys.argv)>2 else 40
nodes={x['id']:x for x in p['nodes']}
dt=collections.Counter()
for s,d in zip(p['samples'],p['timeDeltas']): dt[s]+=d
selfs=collections.Counter(); 
for i,t in dt.items():
    cf=nodes[i]['callFrame']; selfs[cf['functionName'] or '(anon)']+=t
tot=sum(selfs.values())
for k,v in selfs.most_common(n): print(f"{v/1e6:8.1f}s {100*v/tot:5.1f}%  {k[:150]}")
print(f"total {tot/1e6:.1f}s")
