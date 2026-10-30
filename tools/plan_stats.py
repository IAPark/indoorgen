"""Room mix, sizes and narrow-room counts of PLAN_DUMP plans: uv run --with shapely tools/plan_stats.py DIR"""
import json,glob,collections,statistics as S,sys
from shapely import wkt
d0=sys.argv[1]; mix=collections.Counter(); hall=0; nar=collections.Counter(); tot=collections.Counter(); houses=set(); areas=collections.defaultdict(list); sizes=[]; nrooms=collections.Counter()
for f in glob.glob(d0+"/p*.json"):
    d=json.load(open(f)); tw=d["wall_thickness"]
    rs={k:wkt.loads(v) for k,v in d["rooms"].items() if not k.startswith(("exterior","staircase"))}
    c=collections.Counter(k.split("_")[0] for k in rs); nrooms[len(rs)]+=1; sizes.append(sum(p.area for p in rs.values()))
    hall+=c["hallway"]>0
    for t,n in c.items(): mix[(t,n)]+=1
    for k,p in rs.items():
        t=k.split("_")[0]; areas[t].append(p.area); tot[t]+=1
        lim={"bathroom":1.5,"hallway":1.0,"closet":1.0,"utility":1.5,"kitchen":2.1}.get(t,2.4)+tw
        op=p.buffer(-lim/2,join_style=2).buffer(lim/2,join_style=2)
        if (p.area-p.intersection(op).area)/p.area>0.25: nar[t]+=1; houses.add(d["seed"])
print("houses with hallway",hall,"| rooms/house",dict(sorted(nrooms.items())),"| house m2 median %.0f"%S.median(sizes))
for t in sorted(tot): print("%-12s n=%3d median %.1f m2 (%.1f-%.1f)  narrow %d"%(t,tot[t],S.median(areas[t]),min(areas[t]),max(areas[t]),nar[t]))
print("count mix", sorted(mix.items()))
print("houses with a narrow room",len(houses))
