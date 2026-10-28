"""gallery_sheet.py OUT_DIR OUT.png [views per house]: tile images/000.png (and 001...) of every house of a make_dataset run"""
import os, sys
from PIL import Image

out_dir, dst = sys.argv[1], sys.argv[2]
k = int(sys.argv[3]) if len(sys.argv) > 3 else 1
tiles = []
for h in sorted(os.listdir(out_dir)):
    for i in range(k):
        p = os.path.join(out_dir, h, "base", "images", "%03d.png" % i)
        if os.path.exists(p):
            tiles.append(Image.open(p).convert("RGB").resize((320, 240)))
cols = 8
rows = (len(tiles) + cols - 1) // cols
sheet = Image.new("RGB", (cols * 320, rows * 240), (20, 20, 20))
for j, t in enumerate(tiles):
    sheet.paste(t, ((j % cols) * 320, (j // cols) * 240))
sheet.save(dst)
print(len(tiles), "tiles ->", dst)
