#!/usr/bin/env python3
"""glb_merge.py A.glb B.glb OUT.glb [--skip PREFIX,...]: one scene from two (a house generated without the outdoor stage, and gen2 --outdoor-only's
outdoor.glb), with their sidecars (A.json, B.json -> OUT.json) merged as well, so tools that read one scene see both."""
import json, struct, sys

def read(p):
    b = open(p, 'rb').read(); o = 12; j = None; bin_ = b''
    while o < len(b):
        n, t = struct.unpack_from('<II', b, o); c = b[o + 8:o + 8 + n]; o += 8 + n
        if t == 0x4E4F534A: j = json.loads(c)
        elif t == 0x004E4942: bin_ = c
    return j, bin_

def merge(a, abin, b, bbin):
    pad = (-len(abin)) % 8; base = len(abin) + pad
    n = {k: len(a.get(k, [])) for k in ('accessors', 'bufferViews', 'images', 'textures', 'samplers', 'materials', 'meshes', 'nodes')}
    for bv in b.get('bufferViews', []): bv['buffer'] = 0; bv['byteOffset'] = bv.get('byteOffset', 0) + base
    for ac in b.get('accessors', []):
        if 'bufferView' in ac: ac['bufferView'] += n['bufferViews']
    for im in b.get('images', []):
        if 'bufferView' in im: im['bufferView'] += n['bufferViews']
    for tx in b.get('textures', []):
        if 'source' in tx: tx['source'] += n['images']
        if 'sampler' in tx: tx['sampler'] += n['samplers']
    def tex_refs(v):
        if isinstance(v, dict):
            for k, x in v.items():
                if k.endswith('Texture') and isinstance(x, dict) and 'index' in x: x['index'] += n['textures']
                tex_refs(x)
        elif isinstance(v, list):
            for x in v: tex_refs(x)
    for m in b.get('materials', []): tex_refs(m)
    for me in b.get('meshes', []):
        for p in me['primitives']:
            p['attributes'] = {k: v + n['accessors'] for k, v in p['attributes'].items()}
            if 'indices' in p: p['indices'] += n['accessors']
            if 'material' in p: p['material'] += n['materials']
            for t in p.get('targets', []):
                for k in t: t[k] += n['accessors']
    for nd in b.get('nodes', []):
        if 'mesh' in nd: nd['mesh'] += n['meshes']
        if 'children' in nd: nd['children'] = [c + n['nodes'] for c in nd['children']]
    out = dict(a)
    for k in ('accessors', 'bufferViews', 'images', 'textures', 'samplers', 'materials', 'meshes', 'nodes'):
        if a.get(k) or b.get(k): out[k] = a.get(k, []) + b.get(k, [])
    roots = b['scenes'][b.get('scene', 0)]['nodes']
    out['scenes'] = [dict(a['scenes'][a.get('scene', 0)])]; out['scenes'][0]['nodes'] = a['scenes'][a.get('scene', 0)]['nodes'] + [r + n['nodes'] for r in roots]; out['scene'] = 0
    out['extensionsUsed'] = sorted(set(a.get('extensionsUsed', [])) | set(b.get('extensionsUsed', [])))
    allbin = abin + b'\0' * pad + bbin
    out['buffers'] = [{'byteLength': len(allbin)}]
    return out, allbin

def write(p, j, bin_):
    js = json.dumps(j, separators=(',', ':')).encode(); js += b' ' * ((-len(js)) % 4); bin_ += b'\0' * ((-len(bin_)) % 4)
    total = 12 + 8 + len(js) + 8 + len(bin_)
    with open(p, 'wb') as f:
        f.write(struct.pack('<III', 0x46546C67, 2, total)); f.write(struct.pack('<II', len(js), 0x4E4F534A)); f.write(js)
        f.write(struct.pack('<II', len(bin_), 0x004E4942)); f.write(bin_)

def merge_side(sa, sb):
    out = dict(sa); g0 = len(sa.get('graphs', []))
    nodes = []
    for nd in sb.get('nodes', []):
        nd = dict(nd)
        if isinstance(nd.get('graph'), int): nd['graph'] += g0
        nodes.append(nd)
    out['nodes'] = sa.get('nodes', []) + nodes
    out['graphs'] = sa.get('graphs', []) + sb.get('graphs', [])
    out['materials'] = sa.get('materials', []) + sb.get('materials', [])
    out['lights'] = sa.get('lights', []) + sb.get('lights', [])
    if 'outdoor' in sb: out['outdoor'] = sb['outdoor']
    return out

if __name__ == '__main__':
    A, B, O = sys.argv[1:4]
    skip = sys.argv[sys.argv.index('--skip') + 1].split(',') if '--skip' in sys.argv else []
    (a, abin), (b, bbin) = read(A), read(B)
    if skip:  # (B's nodes whose names start so are left out: what each part of the outdoors costs)
        keep = {i for i, nd in enumerate(b['nodes']) if not any(nd.get('name', '').startswith(k) for k in skip)}
        sc = b['scenes'][b.get('scene', 0)]; sc['nodes'] = [r for r in sc['nodes'] if r in keep]
        for nd in b['nodes']:
            if 'children' in nd: nd['children'] = [c for c in nd['children'] if c in keep]
        for i, nd in enumerate(b['nodes']):
            if i not in keep: nd.pop('mesh', None)  # (readers that walk every node, not the scene's tree, skip it too)
    j, bin_ = merge(a, abin, b, bbin); write(O, j, bin_)
    sj = lambda p: json.load(open(p[:-4] + '.json')) if __import__('os').path.exists(p[:-4] + '.json') else {}
    json.dump(merge_side(sj(A), sj(B)), open(O[:-4] + '.json', 'w'))
    print(f'{O}: {len(j["nodes"])} nodes, {len(j["meshes"])} meshes, {len(bin_) / 1e6:.0f} MB')
