#!/usr/bin/env python3
"""meshqc_shots.py REPORT.json SCENE.glb OUT.png [N] [kind] : contact sheet of preview close-ups at the N worst defect spots (<=2 per asset class).
kind: holes (default; by opening area) | tjunc | sliver. Prints the list (index = sheet order) so a tile can be traced back to its node."""
import sys, json, subprocess, os
rep, glb, out = sys.argv[1:4]
N = int(sys.argv[4]) if len(sys.argv) > 4 else 12
kind = sys.argv[5] if len(sys.argv) > 5 else 'holes'
rows = json.load(open(rep))
cand = []
for r in rows:
    if kind == 'holes' and r['defects'].get('holes') and r.get('hole_ex'):
        for e in r['hole_ex']:
            if e.get('exposed'):
                cand.append((e['area'], r, e['c'], e['an'], max(e['ext'], 0.05)))
                break
    elif kind == 'tjunc' and r.get('tjunc_ex'):
        cand.append((r['defects']['tjunc'], r, r['tjunc_ex'][0], [0, 0, 1], 0.1))
    elif kind == 'sliver' and r.get('sliver_ex'):
        cand.append((r['defects']['sliver'], r, r['sliver_ex'][0][0], [0, 0, 1], 0.1))
cand.sort(key=lambda t: -t[0])
seen = {}; pick = []
for c in cand:
    k = c[1]['class'] or c[1]['name']
    if seen.get(k, 0) >= 2: continue
    seen[k] = seen.get(k, 0) + 1; pick.append(c)
    if len(pick) >= N: break
args = []
for i, (m, r, c, n, ext) in enumerate(pick):
    d = min(max(0.35, 1.8 * ext), 1.5)
    n = [float(x) for x in n]
    if abs(n[2]) > 0.95: n = [0.25, 0.1, 0.97 if n[2] > 0 else -0.97]
    eye = [c[k] + n[k] * d for k in range(3)]
    args += ['--at', ','.join('%.3f' % v for v in eye + list(c) + [40])]
    print(i, r['name'], r['class'], 'metric %.4f' % m, 'at', c, 'n', n)
here = os.path.dirname(os.path.abspath(__file__))
subprocess.run([here + '/preview.sh', glb, '--no-ceiling', '--sheet', '--res', '480x270'] + args + ['-o', out], check=True)
