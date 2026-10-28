"""fetch_hdris.py [DIR]: download the outdoor panoramas listed in hdri_selection.json (CC0, Poly Haven, 2k .hdr, ~180 MB) into DIR
(default ~/work/hdri) and write DIR/meta.json (selection group, sun position, mean sky luminance) for dscap.apply_hdri.
Run with the Blender venv (needs cv2): $BLENDER_PY tools/dataset/fetch_hdris.py"""
import os, sys, json, math, shutil, subprocess
import numpy as np
import cv2

d = os.path.expanduser(sys.argv[1] if len(sys.argv) > 1 else "~/work/hdri")
os.makedirs(d, exist_ok=True)
sel = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "hdri_selection.json")))
shutil.copy(os.path.join(os.path.dirname(os.path.abspath(__file__)), "hdri_selection.json"), os.path.join(d, "selection.json"))
group = {k: g for g, ks in sel.items() for k in ks}
for k in group:
    out = os.path.join(d, f"{k}_2k.hdr")
    if not (os.path.exists(out) and os.path.getsize(out) > 1e6):
        subprocess.run(["curl", "-sS", "-m", "120", "-fL", "-o", out, f"https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/2k/{k}_2k.hdr"], check=True)
meta = {}
for k, g in group.items():
    im = cv2.imread(os.path.join(d, f"{k}_2k.hdr"), cv2.IMREAD_ANYDEPTH | cv2.IMREAD_COLOR)
    lum = (0.0722 * im[..., 0] + 0.7152 * im[..., 1] + 0.2126 * im[..., 2]).astype(np.float32)
    small = cv2.resize(lum, (256, 128), interpolation=cv2.INTER_AREA)
    blur = cv2.GaussianBlur(small, (0, 0), 2)
    y, x = np.unravel_index(np.argmax(blur[:64]), blur[:64].shape)  # brightest spot of the upper half = sun
    meta[k] = dict(group=g, sun_u=float((x + 0.5) / 256), sun_el_deg=math.degrees(math.pi / 2 - (y + 0.5) / 128 * math.pi), mean_sky=float(small[:64].mean()), peak=float(blur.max()))
json.dump(meta, open(os.path.join(d, "meta.json"), "w"), indent=1)
print(len(meta), "panoramas in", d)
