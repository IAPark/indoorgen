import json,sys
j=json.load(open(sys.argv[1]))
for s in j['steps']:
    s.pop('expect',None); s.pop('keep',None)
    # keep only step 0 and 1 originals (preprocess, final); drop the second-seed groups
j.pop('final',None)
j['steps']=j['steps'][-2:]  # (placeholder step, if any) is dropped: preprocess + final
keep={s['group'] for s in j['steps']}
# drop groups only reachable from unused steps: recompute reachability
def refs(g):
    out=set()
    for n in j['groups'][g]['nodes']:
        ng=(n.get('p') or {}).get('group')
        if isinstance(ng,str): out.add(ng)
        for k,v in n.items():
            if isinstance(v,str) and v in j['groups']: out.add(v)
    return out
seen=set(); st=list(keep)
while st:
    g=st.pop()
    if g in seen or g not in j['groups']: continue
    seen.add(g); st+=list(refs(g))
j['groups']={k:v for k,v in j['groups'].items() if k in seen}
for k in ('params_a','params_b','objects'): pass
json.dump(j,open(sys.argv[2],'w'),separators=(',',':'))
print(len(j['groups']), list(j['groups'])[:20])
