#!/usr/bin/env python3
"""factor_diff.py A.glb B.glb [--allow CAT:CLASS_REGEX ...] [--list N] [--json]

Compare two generated scenes (glb + sidecar json next to it) node by node and report what changed, grouped by node class:
  same      identical vertex/index bytes and material
  recolored identical geometry, different material colour / emissive / procedural spec
  moved     same vertex count, same shape (rigid motion: sorted vertex distances to the centroid equal) at another place/orientation
  reshaped  geometry changed in another way (different vertex count or shape)
  added / removed   node only in B / only in A
Also compares the sidecar lights, cameras, cloth jobs and `variation` record.
--allow CAT:REGEX (repeatable, REGEX matches the node class, '.*' = any) whitelists expected changes; the exit code is 1 when anything
outside the whitelist changed (so a factor test can assert "only walls recolored"). Sidecar light/camera changes are whitelisted with
--allow lights:.* / --allow cameras:.* / --allow cloth:.* ; the `variation` record is always ignored.
"""
import hashlib, json, math, re, struct, sys, collections


def load(path):
    b = open(path, 'rb').read()
    n = struct.unpack('<I', b[12:16])[0]
    j = json.loads(b[20:20 + n])
    off = 20 + n
    bl = struct.unpack('<I', b[off:off + 4])[0]
    binv = memoryview(b)[off + 8:off + 8 + bl]
    side = {}
    try:
        side = json.load(open(re.sub(r'\.glb$', '.json', path)))
    except Exception:
        pass
    cls = {nd['name']: nd.get('extras', {}).get('class', '') for nd in j['nodes'] if 'mesh' in nd}
    for sn in side.get('nodes', []):
        cls.setdefault(sn['name'], sn.get('class', ''))
    return j, binv, side, cls


def acc_bytes(j, binv, ai):
    a = j['accessors'][ai]
    bv = j['bufferViews'][a['bufferView']]
    sz = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3}[a['type']] * 4
    s = bv['byteOffset'] + a.get('byteOffset', 0)
    return binv[s:s + sz * a['count']], a['count']


def nodes(j, binv):
    out = {}
    seen = collections.Counter()
    for nd in j['nodes']:
        if 'mesh' not in nd:
            continue
        m = j['meshes'][nd['mesh']]
        nm = nd['name']
        seen[nm] += 1
        key = nm if seen[nm] == 1 else '%s~%d' % (nm, seen[nm])
        h = hashlib.md5()
        pos = None
        nv = 0
        mats = []
        for p in m['primitives']:
            pb, c = acc_bytes(j, binv, p['attributes']['POSITION'])
            ib, _ = acc_bytes(j, binv, p['indices'])
            h.update(bytes(pb)); h.update(bytes(ib))
            pos = pb if pos is None else pos
            nv += c
            mats.append(p.get('material'))
        out[key] = dict(hash=h.hexdigest(), pos=pos, nv=nv, mat=mats[0] if mats else None, mesh=m)
    return out


def matkey(j, mi):
    if mi is None:
        return None
    m = j['materials'][mi]
    return json.dumps([m['pbrMetallicRoughness'], m.get('emissiveFactor'), m.get('extensions')], sort_keys=True)


def spec_of(side):
    return {n['name']: json.dumps([n.get('material'), n.get('spec')], sort_keys=True) for n in side.get('nodes', [])}


def shape_sig(pos, nv):
    v = struct.unpack('<%df' % (3 * nv), bytes(pos))
    c = [sum(v[k::3]) / nv for k in range(3)]
    d = sorted(math.dist((v[3 * i], v[3 * i + 1], v[3 * i + 2]), c) for i in range(nv))
    return c, d


def main():
    a = [x for x in sys.argv[1:]]
    allow, lst, as_json = [], 8, False
    pos = []
    i = 0
    while i < len(a):
        if a[i] == '--allow':
            c, r = a[i + 1].split(':', 1); allow.append((c, re.compile(r))); i += 2
        elif a[i] == '--list':
            lst = int(a[i + 1]); i += 2
        elif a[i] == '--json':
            as_json = True; i += 1
        else:
            pos.append(a[i]); i += 1
    ja, ba, sa, ca = load(pos[0])
    jb, bb, sb, cb = load(pos[1])
    na, nb = nodes(ja, ba), nodes(jb, bb)
    spa, spb = spec_of(sa), spec_of(sb)
    res = collections.defaultdict(lambda: collections.defaultdict(list))
    for k in sorted(set(na) | set(nb)):
        cl = cb.get(k) or ca.get(k) or ''
        if k not in na:
            res['added'][cl].append(k); continue
        if k not in nb:
            res['removed'][cl].append(k); continue
        x, y = na[k], nb[k]
        mx, my = matkey(ja, x['mat']), matkey(jb, y['mat'])
        recol = mx != my or spa.get(k) != spb.get(k)
        if x['hash'] == y['hash']:
            res['recolored' if recol else 'same'][cl].append(k)
            continue
        if x['nv'] == y['nv'] and x['nv'] > 0:
            ca_, da = shape_sig(x['pos'], x['nv']); cb_, db = shape_sig(y['pos'], y['nv'])
            if max(abs(p - q) for p, q in zip(da, db)) < 3e-3:
                dd = math.dist(ca_, cb_)
                res['moved'][cl].append('%s (%.3f m)' % (k, dd)); continue
        res['reshaped'][cl].append('%s (nv %d->%d)' % (k, x['nv'], y['nv']))
    # sidecar
    def lights(s):
        return {l['name']: json.dumps(l, sort_keys=True) for l in s.get('lights', [])}
    la, lb = lights(sa), lights(sb)
    side = {'lights': [k for k in set(la) | set(lb) if la.get(k) != lb.get(k)],
            'cameras': [] if sa.get('cameras') == sb.get('cameras') else ['cameras differ'],
            'cloth': [] if sa.get('cloth') == sb.get('cloth') else ['cloth jobs differ']}
    for k, v in side.items():
        if v:
            res[k]['sidecar'] = v
    bad = 0
    summary = {}
    for cat in ['same', 'recolored', 'moved', 'reshaped', 'added', 'removed', 'lights', 'cameras', 'cloth']:
        for cl, ks in res.get(cat, {}).items():
            summary.setdefault(cat, {})[cl or '-'] = len(ks)
            if cat == 'same':
                continue
            if cat in ('lights', 'cameras', 'cloth'):
                ok = any(c == cat for c, r in allow)
            else:
                ok = any(c == cat and r.fullmatch(cl) for c, r in allow)
            if not ok:
                bad += len(ks)
    if as_json:
        print(json.dumps({'summary': summary, 'unexpected': bad}, indent=1))
    else:
        print('A=%s B=%s nodes %d/%d' % (pos[0], pos[1], len(na), len(nb)))
        for cat in ['same', 'recolored', 'moved', 'reshaped', 'added', 'removed', 'lights', 'cameras', 'cloth']:
            if cat not in res:
                continue
            tot = sum(len(v) for v in res[cat].values())
            print('%-9s %5d  %s' % (cat, tot, ', '.join('%s:%d' % (cl or '-', len(v)) for cl, v in sorted(res[cat].items(), key=lambda t: -len(t[1]))[:12])))
            if cat != 'same':
                shown = 0
                for cl, v in res[cat].items():
                    for k in v:
                        if shown < lst:
                            print('            %s [%s]' % (k, cl)); shown += 1
        va, vb = sa.get('variation'), sb.get('variation')
        if va != vb:
            print('variation record differs (ignored for the verdict)')
        print('UNEXPECTED changes: %d' % bad if allow else 'changed nodes (no whitelist): %d' % sum(len(v) for c in res if c not in ('same',) for v in res[c].values()))
    sys.exit(1 if (allow and bad) else 0)


main()
