# Wall-art prints: tone-mapped horizon crops of the HDRI skies (4 aspects x 3 per HDRI, 25 % black-and-white) into ~/work/assets/art_photos.
# Run: Blender -b --factory-startup --python tools/assets/art_crops.py   (render_rust.py uses them for art.Art canvases; RAW_ART=1 opts out)
import bpy, numpy as np, os, glob, random
src = os.path.expanduser("~/work/hdri"); dst = os.path.expanduser("~/work/assets/art_photos")
rnd = random.Random(7)
n = 0
for f in sorted(glob.glob(src + "/*.hdr")):
    im = bpy.data.images.load(f); W, H = im.size
    px = np.array(im.pixels[:], np.float32).reshape(H, W, 4)[::-1, :, :3]  # top row first
    name = os.path.basename(f).replace("_2k.hdr", "")
    for aspect in (0.75, 1.0, 1.33, 1.5):
        for k in range(3):
            ch = int(H * rnd.uniform(0.22, 0.32)); cw = int(ch * aspect)
            cy = int(H * rnd.uniform(0.44, 0.52))  # just above the horizon: sky + land
            cx = rnd.randint(0, W - cw - 1)
            c = px[max(0, cy - ch // 2 - ch // 6): max(0, cy - ch // 2 - ch // 6) + ch, cx:cx + cw]
            ev = 0.18 / max(np.percentile(c.mean(-1), 50), 1e-4)
            c = c * ev; c = c / (1 + c); c = np.clip(c ** (1 / 2.2) * 1.08, 0, 1)
            if rnd.random() < 0.25: c = np.repeat(c.mean(-1, keepdims=True), 3, -1)  # black-and-white print
            out = bpy.data.images.new("crop", cw, ch, float_buffer=False)
            rgba = np.concatenate([c[::-1], np.ones((ch, cw, 1), np.float32)], -1)
            out.pixels = rgba.ravel().tolist(); out.filepath_raw = f"{dst}/{name}_{aspect}_{k}.jpg"; out.file_format = "JPEG"; out.save()
            bpy.data.images.remove(out); n += 1
    bpy.data.images.remove(im)
print("CROPS", n)
