#!/usr/bin/env python3
import json,glob,sys,os,collections,re
d=sys.argv[1]
norm=lambda n:''.join(w.capitalize() for w in n.split('_')[0].split('-'))
t={}
for l in open(os.path.join(d,'times.txt')):
    m=re.match(r'(\w+) (\d+) rc=(\d+) t=(\d+)s',l)
    if m: t[(m[1],m[2])]=int(m[4])
agg=collections.defaultdict(list)
for f in sorted(glob.glob(d+'/*_*.json')):
    b=os.path.basename(f)[:-5]; s,r=b.split('_',1)
    j=json.load(open(f))
    rm=[x for x in j.get('rooms',[]) if norm(x['room'])==r and x['furniture']+x['small_items']>0] or [x for x in j.get('rooms',[]) if norm(x['room'])==r]
    for x in rm[:1]:
        print(f"{r:11s} {s} {x['room']:18s} area={x['area']:5.1f} furn={x['furniture']:2d} small={x['small_items']:3d} fp={x['footprint_frac']:.2f} t={t.get((r,s),'?')}s defects={dict(j['counts'])}")
        agg[r].append((x,t.get((r,s),0),j['counts']))
print("MEAN per type: area furn small fp time")
tot=collections.Counter()
for r,v in agg.items():
    n=len(v); print(f"  {r:11s} n={n} area={sum(x['area'] for x,_,_ in v)/n:5.1f} furn={sum(x['furniture'] for x,_,_ in v)/n:4.1f} small={sum(x['small_items'] for x,_,_ in v)/n:5.1f} fp={sum(x['footprint_frac'] for x,_,_ in v)/n:.2f} time={sum(t for _,t,_ in v)/n:.0f}s")
    for _,_,c in v:
        for k,x in c.items(): tot[k]+=x
print("DEFECTS",dict(tot))
