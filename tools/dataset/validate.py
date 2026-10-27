"""validate.py VARIANT_DIR [--stride 4] [--json OUT]: multi-view consistency of cameras.json + depth (+ RGB).

For every ordered view pair (i, j): unproject the valid depth pixels of view i to world with K_i / c2w_i, project into view j with K_j / w2c_j.
 - depth consistency: among points that land inside view j, the z of the point in camera j must equal depth_j at that pixel
   (inlier if |dz| < 2 % of z or 3 cm); reported: median |dz|/z over inliers, inlier fraction (the rest is occlusion / disocclusion).
 - reprojection error (px): for the inliers, unproject view j's depth at the landing pixel (bilinear, pixel centres) back into view i and
   measure the distance to the original pixel: median / p90 in pixels.  Sub-pixel values prove K, extrinsics and depth agree.
 - photometric: warp RGB_j into view i (inliers) and compare with RGB_i (mean abs grey difference 0..255) against the same warp with
   the pose shifted by 4 px (sensitivity control: a wrong convention makes the real warp as bad as, or worse than, the shifted one).
Exit code 0 when median reprojection < 1.0 px and median depth error < 1 % on every pair that shares >= 5 % overlap."""
import os, sys, json, argparse
import numpy as np
from PIL import Image


def load(d):
    cams = json.load(open(os.path.join(d, "cameras.json")))
    V = []
    for v in cams["views"]:
        dp = os.path.join(d, v["depth"])
        depth = np.load(dp) if dp.endswith(".npy") else None
        img = np.asarray(Image.open(os.path.join(d, v["file"])).convert("L"), np.float32)
        V.append(dict(K=np.array(v["K"]), w2c=np.array(v["w2c"]), c2w=np.array(v["c2w"]), depth=depth, img=img, name=v["file"]))
    return V


def unproject(v, ys, xs):
    z = v["depth"][ys, xs]
    x = (xs + 0.5 - v["K"][0, 2]) / v["K"][0, 0] * z; y = (ys + 0.5 - v["K"][1, 2]) / v["K"][1, 1] * z
    P = np.stack([x, y, z, np.ones_like(z)], 0)
    return (v["c2w"] @ P)[:3].T, z


def project(v, Pw):
    Pc = (v["w2c"] @ np.concatenate([Pw, np.ones((len(Pw), 1))], 1).T)[:3]
    z = Pc[2]; u = Pc[0] / z * v["K"][0, 0] + v["K"][0, 2] - 0.5; w = Pc[1] / z * v["K"][1, 1] + v["K"][1, 2] - 0.5  # pixel-centre indices
    return u, w, z


def bilin(a, u, w):
    H, W = a.shape[:2]
    u0 = np.floor(u).astype(int); w0 = np.floor(w).astype(int); fu = u - u0; fw = w - w0
    u0 = np.clip(u0, 0, W - 2); w0 = np.clip(w0, 0, H - 2)
    g = lambda yy, xx: a[yy, xx]
    return (g(w0, u0) * (1 - fu) * (1 - fw) + g(w0, u0 + 1) * fu * (1 - fw) + g(w0 + 1, u0) * (1 - fu) * fw + g(w0 + 1, u0 + 1) * fu * fw)


def pair(vi, vj, stride):
    H, W = vi["depth"].shape
    ys, xs = np.mgrid[stride // 2:H:stride, stride // 2:W:stride]; ys = ys.ravel(); xs = xs.ravel()
    ok = vi["depth"][ys, xs] > 0; ys, xs = ys[ok], xs[ok]
    Pw, zi = unproject(vi, ys, xs)
    u, w, zj = project(vj, Pw)
    Hj, Wj = vj["depth"].shape
    inb = (zj > 0.05) & (u >= 0) & (u <= Wj - 1.001) & (w >= 0) & (w <= Hj - 1.001)
    res = dict(overlap=float(inb.sum() / max(len(ys), 1)))
    if inb.sum() < 50: return res
    u, w, zj, Pw, ys, xs = u[inb], w[inb], zj[inb], Pw[inb], ys[inb], xs[inb]
    # nearest depth for the comparison (bilinear would blend across depth edges), restricted to all-4-neighbours-similar pixels
    dj = vj["depth"]; un = np.clip(np.rint(u).astype(int), 0, Wj - 1); wn = np.clip(np.rint(w).astype(int), 0, Hj - 1)
    dn = dj[wn, un]
    u0 = np.clip(np.floor(u).astype(int), 0, Wj - 2); w0 = np.clip(np.floor(w).astype(int), 0, Hj - 2)
    nb = np.stack([dj[w0, u0], dj[w0, u0 + 1], dj[w0 + 1, u0], dj[w0 + 1, u0 + 1]], 0)
    smooth = (nb.min(0) > 0) & ((nb.max(0) - nb.min(0)) < 0.02 * np.maximum(nb.max(0), 1e-3))  # no depth edge under the 2x2 footprint
    dz = np.abs(zj - dn); inl = (dn > 0) & (dz < np.maximum(0.02 * zj, 0.03))
    res["depth_inlier_frac"] = float(inl.sum() / len(zj))
    m = inl & smooth
    if m.sum() < 50: return res
    res["depth_err_rel_med"] = float(np.median(dz[m] / zj[m]))
    dbil = bilin(dj, u[m], w[m])
    xj = (u[m] + 0.5 - vj["K"][0, 2]) / vj["K"][0, 0] * dbil; yj = (w[m] + 0.5 - vj["K"][1, 2]) / vj["K"][1, 1] * dbil
    Pj = (vj["c2w"] @ np.stack([xj, yj, dbil, np.ones_like(dbil)], 0))[:3].T
    ub, wb, _ = project(vi, Pj)
    e = np.hypot(ub - xs[m], wb - ys[m])
    res["reproj_px_med"] = float(np.median(e)); res["reproj_px_p90"] = float(np.percentile(e, 90))
    # photometric
    gi = vi["img"][ys[m], xs[m]]; gj = bilin(vj["img"], u[m], w[m])
    tex = np.abs(vi["img"][np.clip(ys[m] + 2, 0, H - 1), xs[m]] - vi["img"][np.clip(ys[m] - 2, 0, H - 1), xs[m]]) + np.abs(vi["img"][ys[m], np.clip(xs[m] + 2, 0, W - 1)] - vi["img"][ys[m], np.clip(xs[m] - 2, 0, W - 1)])
    t = tex > 8  # textured pixels only, where misalignment shows
    if t.sum() > 50:
        res["photo_abs_med"] = float(np.median(np.abs(gi - gj)[t]))
        gs = bilin(vj["img"], np.clip(u[m] + 4, 0, Wj - 1.01), w[m])
        res["photo_abs_med_shift4px"] = float(np.median(np.abs(gi - gs)[t]))
    return res


def align(v, R=4):
    """RGB vs depth edge alignment: shift of the depth-edge map that maximises its overlap with the image gradient (expected (0, 0)):
    proves the Cycles camera (lens/pose/resolution) is the camera K / c2w describe, independent of the depth's own consistency"""
    g = v["img"]; d = np.log(np.maximum(v["depth"], 0.05)) * (v["depth"] > 0)
    gm = np.hypot(np.diff(g, axis=1, prepend=g[:, :1]), np.diff(g, axis=0, prepend=g[:1]))
    de = (np.hypot(np.diff(d, axis=1, prepend=d[:, :1]), np.diff(d, axis=0, prepend=d[:1])) > 0.06).astype(np.float32)
    H, W = g.shape; best = None; sc = {}
    c = (slice(R, H - R), slice(R, W - R))
    for dy in range(-R, R + 1):
        for dx in range(-R, R + 1):
            s_ = float((np.roll(np.roll(gm, dy, 0), dx, 1)[c] * de[c]).sum()); sc[(dy, dx)] = s_
            if best is None or s_ > sc[best]: best = (dy, dx)
    return best, sc[best] / max(sc[(0, 0)], 1e-9)


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("dir"); ap.add_argument("--stride", type=int, default=4); ap.add_argument("--json")
    a = ap.parse_args()
    V = load(a.dir); out = []; bad = 0
    for i in range(len(V)):
        for j in range(len(V)):
            if i == j: continue
            r = pair(V[i], V[j], a.stride); r.update(i=i, j=j); out.append(r)
            ok = r["overlap"] < 0.05 or ("reproj_px_med" in r and r["reproj_px_med"] < 1.0 and r["depth_err_rel_med"] < 0.01)
            bad += not ok
            print("%d->%d overlap %.2f inl %s depth_err %s reproj %s/%s photo %s (shift4 %s) %s" % (i, j, r["overlap"], *("%.3f" % r[k] if k in r else "-" for k in ("depth_inlier_frac",)),
                  *("%.4f" % r[k] if k in r else "-" for k in ("depth_err_rel_med",)), *("%.2f" % r[k] if k in r else "-" for k in ("reproj_px_med", "reproj_px_p90", "photo_abs_med", "photo_abs_med_shift4px")), "ok" if ok else "BAD"))
    al = [align(v) for v in V]; print("ALIGN best (dy,dx) per view, score/score(0,0):", [(b, round(r, 3)) for b, r in al])
    bad += sum(abs(b[0]) > 1 or abs(b[1]) > 1 for b, _ in al)
    ov = [r for r in out if "reproj_px_med" in r]
    summ = dict(rgb_depth_best_shift=[list(b) for b, _ in al], pairs=len(out), pairs_with_overlap=len(ov), bad=bad,
                reproj_px_med=float(np.median([r["reproj_px_med"] for r in ov])) if ov else None,
                depth_err_rel_med=float(np.median([r["depth_err_rel_med"] for r in ov])) if ov else None,
                photo_abs_med=float(np.median([r["photo_abs_med"] for r in ov if "photo_abs_med" in r])) if any("photo_abs_med" in r for r in ov) else None,
                photo_abs_med_shift4px=float(np.median([r["photo_abs_med_shift4px"] for r in ov if "photo_abs_med" in r])) if any("photo_abs_med" in r for r in ov) else None)
    print("SUMMARY", json.dumps(summ))
    if a.json: json.dump(dict(summary=summ, pairs=out), open(a.json, "w"), indent=1)
    sys.exit(1 if bad or not ov else 0)


if __name__ == "__main__":
    main()
