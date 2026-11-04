# variety_sheet.py OUT SEEDS...: OUT/sheet.jpg (living rooms, then bedrooms) and how far the houses differ: the spread of each house's mean colour, across houses and within one
import glob, json, os, sys, statistics as st
from PIL import Image, ImageDraw
out, seeds = sys.argv[1], sys.argv[2:]
W, H, C = 384, 216, 5
def cols(f):
    im = Image.open(f).convert('RGB').resize((16, 9))
    px = list(im.getdata()); n = len(px)
    m = [sum(p[i] for p in px) / n / 255 for i in range(3)]
    sat = sum((max(p) - min(p)) / max(max(p), 1) for p in px) / n
    return m, sat
rows = {'living-room': [], 'bedroom': []}
def style(s):
    try: return json.load(open(f'{out}/h{s}.json')).get('style') or ''
    except (OSError, ValueError): return ''
house = {}
for s in seeds:
    for k in rows:
        f = sorted(glob.glob(f'{out}/v{s}/*{k}_*.png'))
        if f: rows[k].append((s, f[0]))
    house[s] = [cols(f) for f in sorted(glob.glob(f'{out}/v{s}/*.png'))]
n = sum((len(v) + C - 1) // C for v in rows.values())
sheet = Image.new('RGB', (W * C, H * n)); d = ImageDraw.Draw(sheet); y = 0
for k, v in rows.items():
    for i, (s, f) in enumerate(v):
        x = (i % C) * W; yy = y + (i // C) * H
        sheet.paste(Image.open(f).convert('RGB').resize((W, H)), (x, yy)); d.text((x + 4, yy + 3), f'{s} {k} {style(s)}', fill=(255, 255, 0))
    y += (len(v) + C - 1) // C * H
sheet.save(f'{out}/sheet.jpg', quality=85)
# how different two houses' colours are, as distributions: a sofa changing colour in a white room moves this and hardly moves the mean. 0 same, 1 no colour shared
def hist(f):
    h = {}
    for r, g, b in Image.open(f).convert('RGB').resize((64, 36)).getdata(): k = (r >> 5, g >> 5, b >> 5); h[k] = h.get(k, 0) + 1
    return h
def house_hist(fs):
    t = {}
    for f in fs:
        for k, v in hist(f).items(): t[k] = t.get(k, 0) + v
    n = sum(t.values()) or 1
    return {k: v / n for k, v in t.items()}
hh = [house_hist(sorted(glob.glob(f'{out}/v{s}/*.png'))) for s in seeds if glob.glob(f'{out}/v{s}/*.png')]
pd = [sum(abs(a.get(k, 0) - b.get(k, 0)) for k in set(a) | set(b)) / 2 for i, a in enumerate(hh) for b in hh[:i]]
def mean(a): return [sum(x[i] for x in a) / len(a) for i in range(3)]
hs = {s: mean([c[0] for c in v]) for s, v in house.items() if v}
across = (sum(st.pstdev(m[i] for m in hs.values()) ** 2 for i in range(3))) ** 0.5
within = st.mean((sum(st.pstdev(c[0][i] for c in v) ** 2 for i in range(3))) ** 0.5 for v in house.values() if len(v) > 1)
sats = [st.mean(c[1] for c in v) for v in house.values() if v]
print(f'{len(hs)} houses; palette distance {st.mean(pd):.3f}; colour spread across houses {across:.3f}, within a house {within:.3f}; mean saturation {st.mean(sats):.3f} (min {min(sats):.3f}, max {max(sats):.3f}); {out}/sheet.jpg')
