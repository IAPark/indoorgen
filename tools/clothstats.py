"""clothstats.py SCENE.glb [CLOTH_OVERLAY.glb] : shape numbers of every bed's linen, to calibrate a cloth solver against Blender.
Per bed (mattress #3, comforter #4, throw #5 of each BedFactory): comforter drop (mattress top - lowest point) and loft (highest point -
mattress top), quilting (std of the comforter's top-surface height over the mattress after removing a smooth trend), throw share of
vertices above the mattress top and throw height above it. Without an overlay the scene's own meshes are used (Rust-simulated cloth).
Run with the Blender-5.2 venv: $BLENDER_PY tools/clothstats.py SCENE.glb [OVERLAY.glb]"""
import os, sys
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cloth_only import Glb


def stats(mesh):
    beds = sorted({n.split("#")[0] for n in mesh("__names__") if n.endswith("BedFactory#3")})
    out = []
    for b in beds:
        try:
            mv, _ = mesh(b + "#3")
            cv, _ = mesh(b + "#4")
            tv, _ = mesh(b + "#5")
        except KeyError:
            continue
        top = mv[:, 2].max()
        lo, hi = mv[:, :2].min(0), mv[:, :2].max(0)
        inside = (cv[:, 0] > lo[0]) & (cv[:, 0] < hi[0]) & (cv[:, 1] > lo[1]) & (cv[:, 1] < hi[1]) & (cv[:, 2] > top - 0.02)
        q = 0.0
        if inside.sum() > 50:
            p = cv[inside]
            # top surface: highest vertex per 2 cm cell, minus a quadratic trend
            cell = np.floor((p[:, :2] - lo) / 0.02).astype(int)
            key = cell[:, 0] * 10000 + cell[:, 1]
            order = np.lexsort((-p[:, 2], key))
            first = np.r_[True, key[order][1:] != key[order][:-1]]
            s = p[order][first]
            A = np.c_[np.ones(len(s)), s[:, 0], s[:, 1], s[:, 0] ** 2, s[:, 1] ** 2, s[:, 0] * s[:, 1]]
            coef, *_ = np.linalg.lstsq(A, s[:, 2], rcond=None)
            q = float(np.std(s[:, 2] - A @ coef))
        out.append(dict(bed=b, drop=top - cv[:, 2].min(), loft=cv[:, 2].max() - top, quilt=q, throw_on_top=float((tv[:, 2] > top).mean()), throw_h=tv[:, 2].max() - top))
    return out


def main():
    g = Glb(sys.argv[1])
    o = Glb(sys.argv[2]) if len(sys.argv) > 2 else None

    def mesh(n):
        if n == "__names__":
            return list(g.by_name)
        if o is not None and n in o.by_name:
            return o.mesh(n)
        return g.mesh(n)

    for r in stats(mesh):
        print(f"{r['bed']:20s} comforter drop {r['drop']*100:5.1f} cm loft {r['loft']*100:5.1f} cm quilt {r['quilt']*1000:5.1f} mm | throw on top {100*r['throw_on_top']:5.1f}% height {r['throw_h']*100:5.1f} cm")


if __name__ == "__main__":
    main()
