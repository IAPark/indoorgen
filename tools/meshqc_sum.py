#!/usr/bin/env python3
"""meshqc_sum.py DIR [--base DIR0] : aggregate meshqc JSON files: per defect class (nodes affected / total items) and per asset class."""
import sys, json, glob, os, re, collections
CL = ['holes', 'holes_open', 'cracks', 'nonmanif', 'tjunc', 'degenerate', 'sliver', 'flipped', 'inconsistent', 'dupface', 'xdup', 'nan', 'empty']
def load(d):
    rows = []
    for f in sorted(glob.glob(os.path.join(d, '*.json'))):
        for r in json.load(open(f)):
            r['scene'] = os.path.basename(f)[:-5]
            rows.append(r)
    return rows
def acls(r):
    return r['class'] or re.sub(r'^\d+_', '', re.sub(r'#\d+$', '', r['name'])).rstrip('0123456789.')
def agg(rows):
    nodes = collections.Counter(); items = collections.Counter(); bycls = collections.defaultdict(collections.Counter); ncls = collections.Counter()
    hl = 0.0
    for r in rows:
        c = acls(r); ncls[c] += 1
        for k in CL:
            v = r['defects'].get(k, 0)
            if v:
                nodes[k] += 1; items[k] += v; bycls[c][k] += 1
        hl += r.get('hole_len', 0)
    return nodes, items, bycls, ncls, hl
def main():
    d = sys.argv[1]; base = sys.argv[sys.argv.index('--base') + 1] if '--base' in sys.argv else None
    rows = load(d); n, i, b, nc, hl = agg(rows)
    bn = bi = bb = None
    if base:
        brs = load(base); bn, bi, bb, bnc, bhl = agg(brs)
    print('scenes', len(set(r['scene'] for r in rows)), 'nodes', len(rows), 'hole_area_m2 %.2f' % hl, ('(before %.1f)' % bhl if base else ''))
    print('%-14s %12s %12s' % ('defect', 'nodes', 'items'))
    for k in CL:
        print('%-14s %5d%7s %7d%7s' % (k, n[k], ('(%d)' % bn[k]) if base else '', i[k], ('(%d)' % bi[k]) if base else ''))
    print('\nper asset class (nodes with defect / nodes):')
    allc = sorted(set(b) | (set(bb) if bb else set()), key=lambda c: -sum(b[c].values()))
    for c in allc[:int(os.environ.get('TOPC', 30))]:
        print('%-34s n=%-4d' % (c, nc[c]), ' '.join('%s=%d%s' % (k, b[c][k], ('(%d)' % bb[c][k]) if bb else '') for k in CL if b[c][k] or (bb and bb[c][k])))
main()
