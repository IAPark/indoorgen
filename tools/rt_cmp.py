#!/usr/bin/env python3
"""rt_cmp.py REF_DIR OUT_DIR [--res WxH] [--rt fast|full]: draw every reference view with the previewer and put it beside Blender's: OUT_DIR/cmp.jpg, with per-view numbers.
REF_DIR holds SEED_VIEW.png, SEED_VIEW.cam.json, hSEED.glb, hSEED.json (tools/render/... render_refs). The numbers: mean abs difference of sRGB colour (0-255) on a blurred
copy (the light), and of the full images; mean luminance of each. They are a check beside the eye, not a substitute."""
import glob, os, subprocess, sys, time
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageStat
ref, out = sys.argv[1:3]
mode = sys.argv[sys.argv.index('--rt') + 1] if '--rt' in sys.argv else 'full'
env = dict(os.environ)
views = sorted(f[:-9] for f in map(os.path.basename, glob.glob(ref + '/*.cam.json')))
only = os.environ.get('ONLY')
views = [v for v in views if os.path.exists(f'{ref}/{v}.png')]
if only: views = [v for v in views if any(o in v for o in only.split(','))]
preview = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'preview.sh')
os.makedirs(out, exist_ok=True)
W, H = 640, 360
rows = []
tot = [0, 0]
for v in views:
    s = v.split('_')[0]
    p = f'{out}/{v}_{mode.split(":")[0]}.png'
    t = time.time()
    # (--rt bake[:SECONDS]: the baked mode. RT_CMP_PRE: a command to run the previewer under, as a lock for timing)
    args = ['--bake', 'still' + mode[4:]] if mode.startswith('bake') else ['--rt', mode]
    r = subprocess.run(os.environ.get('RT_CMP_PRE', '').split() + [preview, f'{ref}/h{s}.glb', '--cam', f'{ref}/{v}.cam.json', '--res', '960x540', *args, '-v', '-o', p], env=env, capture_output=True, text=True)
    traced = [l for l in r.stderr.splitlines() if 'exposure x' in l or 'baked in' in l]
    if not os.path.exists(p): print(v, 'failed', r.stderr[-400:]); continue
    a, b = Image.open(f'{ref}/{v}.png').convert('RGB').resize((960, 540)), Image.open(p).convert('RGB')
    blur = lambda im: im.filter(ImageFilter.GaussianBlur(12))
    mean = lambda im: sum(ImageStat.Stat(im).mean) / 3
    dl, df = mean(ImageChops.difference(blur(a), blur(b))), mean(ImageChops.difference(a, b))
    la, lb = sum(ImageStat.Stat(a.convert('L')).mean), sum(ImageStat.Stat(b.convert('L')).mean)
    tot[0] += dl; tot[1] += df
    print(f'{v:32} light {dl:5.1f}  full {df:5.1f}  lum blender {la:5.1f} rt {lb:5.1f}  {" ".join(x.strip() for x in traced)[:90]}')
    rows.append((v, a, b))
if rows:
    print(f'mean light {tot[0] / len(rows):.1f}  full {tot[1] / len(rows):.1f}')
    sheet = Image.new('RGB', (W * 2, H * len(rows))); d = ImageDraw.Draw(sheet)
    for i, (v, a, b) in enumerate(rows):
        sheet.paste(a.resize((W, H)), (0, i * H)); sheet.paste(b.resize((W, H)), (W, i * H)); d.text((4, i * H + 3), f'{v} Blender', fill=(255, 255, 0)); d.text((W + 4, i * H + 3), 'rt ' + mode, fill=(255, 255, 0))
    sheet.save(f'{out}/cmp.jpg', quality=88)
