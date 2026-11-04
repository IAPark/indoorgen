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
def mean(a): return [sum(x[i] for x in a) / len(a) for i in range(3)]
hs = {s: mean([c[0] for c in v]) for s, v in house.items() if v}
across = (sum(st.pstdev(m[i] for m in hs.values()) ** 2 for i in range(3))) ** 0.5
within = st.mean((sum(st.pstdev(c[0][i] for c in v) ** 2 for i in range(3))) ** 0.5 for v in house.values() if len(v) > 1)
sats = [st.mean(c[1] for c in v) for v in house.values() if v]
print(f'{len(hs)} houses; colour spread across houses {across:.3f}, within a house {within:.3f}; mean saturation {st.mean(sats):.3f} (min {min(sats):.3f}, max {max(sats):.3f}); {out}/sheet.jpg')
