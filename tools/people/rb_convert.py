#!/usr/bin/env python3
"""Convert Rocketbox TGA textures in a dir to jpg/png and delete the TGAs. usage: rb_convert.py <dir>"""
import os, sys
from PIL import Image
d = sys.argv[1]
for f in sorted(os.listdir(d)):
    if not f.lower().endswith(".tga"):
        continue
    p = os.path.join(d, f)
    base = f[:-4]
    if "wrinkle" in base.lower():
        os.remove(p); continue
    im = Image.open(p)
    lo = base.lower()
    if "color" in lo and "opacity" not in lo or "specular" in lo:
        im.convert("RGB").save(os.path.join(d, base + ".jpg"), quality=92)
    elif "opacity" in lo:
        # hair card texture: rgb colour + alpha
        im.convert("RGBA").save(os.path.join(d, base + ".png"), optimize=True)
    else:  # normal maps
        im.convert("RGB").save(os.path.join(d, base + ".png"), optimize=True)
    os.remove(p)
