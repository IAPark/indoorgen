#!/usr/bin/env python3
"""cams_sheet.py OUT.png DIR : contact sheet of DIR/*_camN.png with labels"""
import sys, glob, os
from PIL import Image, ImageDraw
fs = sorted(glob.glob(os.path.join(sys.argv[2], "*_cam*.png")))
cols = 3; w, h = 384, 216
rows = (len(fs) + cols - 1) // cols
im = Image.new("RGB", (cols * w, rows * (h + 14)), "white"); d = ImageDraw.Draw(im)
for i, f in enumerate(fs):
    x, y = (i % cols) * w, (i // cols) * (h + 14)
    im.paste(Image.open(f).convert("RGB").resize((w, h)), (x, y + 14)); d.text((x + 2, y + 1), os.path.basename(f)[:-4], fill="black")
im.save(sys.argv[1]); print(len(fs), "shots ->", sys.argv[1])
