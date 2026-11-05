struct P { inv_vp: mat4x4<f32>, cam: vec4<f32>, sun_dir: vec4<f32>, sun_col: vec4<f32>, dims: vec4<u32>, misc: vec4<u32>, misc2: vec4<u32>, scale: vec4<f32> };
struct N { lo: vec3<f32>, a: u32, hi: vec3<f32>, b: u32 };
struct T { v0: vec3<f32>, mat: u32, e1: vec4<f32>, e2: vec4<f32> };
struct M { albedo: vec4<f32>, emissive: vec4<f32> };
struct L { pos: vec4<f32>, col: vec4<f32> };
const PI = 3.14159265;

// ---- shared
fn world_of(px: vec2<i32>, d: f32, dims: vec2<f32>, inv_vp: mat4x4<f32>) -> vec3<f32> {
  let ndc = vec2<f32>((f32(px.x) + 0.5) / dims.x * 2.0 - 1.0, 1.0 - (f32(px.y) + 0.5) / dims.y * 2.0);
  let w = inv_vp * vec4<f32>(ndc, d, 1.0);
  return w.xyz / w.w;
}
fn sky_uv(d: vec3<f32>, sw: u32, sh: u32) -> vec2<f32> {
  let az = fract(atan2(d.y, d.x) / (2.0 * PI));
  let v = sqrt(asin(clamp(d.z, 0.0, 1.0)) / (PI * 0.5));
  return vec2<f32>(az * f32(sw) - 0.5, v * f32(sh) - 0.5);
}

// ---- trace
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> nodes: array<N>;
@group(0) @binding(2) var<storage, read> tris: array<T>;
@group(0) @binding(3) var<storage, read> mats: array<M>;
@group(0) @binding(4) var<storage, read> lights: array<L>;
@group(0) @binding(5) var<storage, read> sky_t: array<vec4<f32>>;
@group(0) @binding(6) var g_depth: texture_depth_2d;
@group(0) @binding(7) var g_nrm: texture_2d<f32>;
@group(0) @binding(8) var g_alb: texture_2d<f32>;
@group(0) @binding(12) var irr_done: texture_2d<f32>;
@group(0) @binding(13) var m_lit: texture_2d<f32>;
@group(0) @binding(9) var out_irr: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var out_spec: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var out_glint: texture_storage_2d<rgba16float, write>;
@group(0) @binding(15) var out_lamp: texture_storage_2d<rgba16float, write>;
@group(0) @binding(16) var lamp_done: texture_2d<f32>;
@group(0) @binding(17) var g_mat: texture_2d<f32>;
struct W { c: vec4<f32>, u: vec4<f32>, v: vec4<f32>, n: vec4<f32> };
@group(0) @binding(11) var<storage, read> wins: array<W>;
// ReSTIR GI, within one frame: each pixel keeps one of its first bounces, chosen by how much light it brought (Ouyang et al. 2021)
struct R { pos: vec3<f32>, wsum: f32, nrm: vec3<f32>, m: f32, rad: vec3<f32>, lp: f32 };
@group(0) @binding(18) var<storage, read_write> resv: array<R>;
// the light cache: bounced light arriving at a surface. Not its colour, which whoever reads it has; not the sun, lamps or windows, whose sharp shadows stay traced; one cell per 20 cm, side of a surface and part, so nothing leaks through a wall. Filled by the paths themselves, at every hit but the last: what a path gathers after a hit, over what reached that hit, is a sample of this. From a later hit it has fewer bounces left, so what those would have brought is estimated from the path's last light and the colour there, to keep Blender's count: no cell reads another while filling, or it would run on without end
@group(0) @binding(19) var<storage, read_write> lc_cells: array<atomic<u32>>;
@group(0) @binding(20) var<storage, read_write> lc_acc: array<array<atomic<u32>, 4>>;
@group(0) @binding(21) var<storage, read_write> lc_out: array<vec4<f32>>;
const LC_SIZE = 0.2; const LC_MASK = 262143u;
const LC_MAX = 4096.0; // (paths a cell takes: then it has settled, and the light of a house doesn't change)
fn lc_key(q: vec3<f32>, ng: vec3<f32>, part: u32) -> u32 { return lc_key_s(q, ng, part, LC_SIZE); }
fn lc_key_s(q: vec3<f32>, ng: vec3<f32>, part: u32, size: f32) -> u32 {
  let c = vec3<i32>(floor(q / size)); let an = abs(ng);
  var side = 2u; if (an.x >= an.y && an.x >= an.z) { side = 0u; } else if (an.y >= an.z) { side = 1u; }
  side = side * 2u + select(0u, 1u, ng[side] < 0.0);
  var h = u32(c.x) * 73856093u ^ u32(c.y) * 19349663u ^ u32(c.z) * 83492791u ^ part * 2654435761u ^ side * 3266489917u;
  h ^= h >> 15u; h *= 2246822519u; h ^= h >> 13u;
  return max(h, 1u);
}
// the slot of a cell, or the table's size if it has none. Paths make the cells they land in: only what rays reach takes a slot
fn lc_find(q: vec3<f32>, ng: vec3<f32>, part: u32) -> u32 { return lc_find_k(lc_key(q, ng, part)); }
fn lc_find_k(k: u32) -> u32 {
  for (var i = 0u; i < 8u; i++) {
    let sl = (k + i) & LC_MASK;
    loop {
      let ck = atomicLoad(&lc_cells[sl]);
      if (ck == k) { return sl; } if (ck != 0u) { break; }
      let r = atomicCompareExchangeWeak(&lc_cells[sl], 0u, k);
      if (r.exchanged || r.old_value == k) { return sl; } if (r.old_value != 0u) { break; }
    }
  }
  return LC_MASK + 1u;
}
// the same, for a read that wouldn't fill the cell
fn lc_look(q: vec3<f32>, ng: vec3<f32>, part: u32) -> u32 { return lc_look_k(lc_key(q, ng, part)); }
fn lc_look_k(k: u32) -> u32 {
  for (var i = 0u; i < 8u; i++) {
    let sl = (k + i) & LC_MASK; let ck = atomicLoad(&lc_cells[sl]);
    if (ck == k) { return sl; } if (ck == 0u) { break; }
  }
  return LC_MASK + 1u;
}
// what a cell has so far, while paths are still adding to it
fn lc_live(sl: u32) -> vec4<f32> {
  if (sl > LC_MASK) { return vec4<f32>(0.0); }
  let n = atomicLoad(&lc_acc[sl][3]); if (n == 0u) { return vec4<f32>(0.0); }
  return vec4<f32>(f32(atomicLoad(&lc_acc[sl][0])), f32(atomicLoad(&lc_acc[sl][1])), f32(atomicLoad(&lc_acc[sl][2])), f32(n)) / vec4<f32>(vec3<f32>(LC_FIX * f32(n)), 1.0);
}
// a path's light into a cell
fn lc_add(sl: u32, l: vec3<f32>) {
  if (lc_out[sl].w >= LC_MAX) { return; }
  let c = clamp(l, vec3<f32>(0.0), vec3<f32>(64.0)) * LC_FIX;
  atomicAdd(&lc_acc[sl][0], u32(c.x)); atomicAdd(&lc_acc[sl][1], u32(c.y)); atomicAdd(&lc_acc[sl][2], u32(c.z)); atomicAdd(&lc_acc[sl][3], 1u);
}
// a place near the point, on its surface: read point by point the cells would show as blocks, and the filter takes this noise out
fn lc_jit(q: vec3<f32>, ng: vec3<f32>) -> vec3<f32> { let b = basis(ng); return q + (b[0] * (rnd() - 0.5) + b[1] * (rnd() - 0.5)) * LC_SIZE; }
fn lc_on() -> bool { return (u32(p.cam.w) & 8u) != 0u; }
// what the paths brought so far, as an average: each cell counts as filled from LC_MIN paths
const LC_MIN = 8.0; const LC_FIX = 1024.0;
@compute @workgroup_size(64) fn lc_resolve(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x; if (i > LC_MASK) { return; }
  let n = atomicLoad(&lc_acc[i][3]); if (n == 0u) { return; }
  lc_out[i] = vec4<f32>(f32(atomicLoad(&lc_acc[i][0])), f32(atomicLoad(&lc_acc[i][1])), f32(atomicLoad(&lc_acc[i][2])), f32(n)) / vec4<f32>(vec3<f32>(LC_FIX * f32(n)), 1.0);
}
var<private> g1_pos = vec3<f32>(0.0); var<private> g1_nrm = vec3<f32>(0.0);

fn dfg_ab(rough: f32, nv: f32) -> vec2<f32> {
  // (GGX with Smith height-correlated visibility, integrated numerically and fitted: A is F0's share, B the rest. Karis's fit runs 10-20% low on rough metal, by amounts that change with roughness)
  let r = clamp(rough, 0.03, 1.0); let n = sqrt(clamp(nv, 0.02, 1.0));
  return clamp(vec2<f32>(((((((0.0) * r + ((0.0) * n + -1.06238)) * r + (((0.0) * n + -1.13516) * n + 5.09599)) * r + ((((0.0) * n + 1.20494) * n + 2.65569) * n + -8.09432)) * r + (((((0.0) * n + 3.69451) * n + -13.18039) * n + 7.02040) * n + 2.75210)) * r + ((((((0.0) * n + -9.40946) * n + 20.37376) * n + -6.83342) * n + -5.76550) * n + 2.02431)) * r + (((((((0.0) * n + 0.84500) * n + 4.33061) * n + -14.41393) * n + 11.26147) * n + -1.06780) * n + 0.00164), ((((((0.0) * r + ((0.0) * n + 1.14387)) * r + (((0.0) * n + 4.14001) * n + -6.51648)) * r + ((((0.0) * n + -3.96983) * n + -3.79358) * n + 9.03557)) * r + (((((0.0) * n + -0.08939) * n + 9.49092) * n + -6.93370) * n + -2.28677)) * r + ((((((0.0) * n + 7.71315) * n + -19.64934) * n + 9.94136) * n + 3.90567) * n + -2.15063)) * r + (((((((0.0) * n + -0.51982) * n + -4.52137) * n + 13.94099) * n + -11.04725) * n + 1.21913) * n + 0.95358)), vec2<f32>(0.0), vec2<f32>(1.0));
}
fn sky_at(d: vec3<f32>) -> vec3<f32> {
  // (below the horizon Blender's sky does not stop: the horizon's colour, gone by 24 degrees down, as measured from it. A warm fill for a low sun)
  var dd = d; var fade = 1.0;
  if (d.z <= 0.0) {
    let el = asin(clamp(d.z, -1.0, 0.0));
    fade = max(1.0 + el * 7.5 / PI, 0.0); fade = fade * fade * fade;
    if (fade <= 0.0) { return vec3<f32>(0.0); }
    dd = normalize(vec3<f32>(d.x, d.y, 1e-4));
  }
  let sw = i32(p.misc.w); let sh = i32(p.misc2.x);
  let uv = sky_uv(dd, u32(sw), u32(sh));
  let x0 = i32(floor(uv.x)); let y0 = clamp(i32(floor(uv.y)), 0, sh - 1); let y1 = min(y0 + 1, sh - 1);
  let fx = uv.x - floor(uv.x); let fy = clamp(uv.y - floor(uv.y), 0.0, 1.0);
  let xa = (x0 + sw) % sw; let xb = (x0 + 1 + sw) % sw;
  let a = mix(sky_t[y0 * sw + xa].rgb, sky_t[y0 * sw + xb].rgb, fx);
  let b = mix(sky_t[y1 * sw + xa].rgb, sky_t[y1 * sw + xb].rgb, fx);
  return mix(a, b, fy) * p.scale.y * fade;
}

var<private> seed: u32;
fn rnd() -> f32 {
  seed = seed * 747796405u + 2891336453u;
  var w = ((seed >> ((seed >> 28u) + 4u)) ^ seed) * 277803737u;
  w = (w >> 22u) ^ w;
  return f32(w) / 4294967296.0;
}
fn basis(n: vec3<f32>) -> mat3x3<f32> {
  let s = select(-1.0, 1.0, n.z >= 0.0);
  let a = -1.0 / (s + n.z); let b = n.x * n.y * a;
  return mat3x3<f32>(vec3<f32>(1.0 + s * n.x * n.x * a, s * b, -s * n.x), vec3<f32>(b, s + n.y * n.y * a, -n.y), n);
}
fn cosine(n: vec3<f32>) -> vec3<f32> {
  let u = rnd(); let v = rnd();
  let r = sqrt(u); let a = 2.0 * PI * v;
  return basis(n) * vec3<f32>(r * cos(a), r * sin(a), sqrt(max(1.0 - u, 0.0)));
}
fn cone(d: vec3<f32>, angle: f32) -> vec3<f32> {
  let u = rnd(); let v = rnd();
  let r = angle * sqrt(u); let a = 2.0 * PI * v;
  return normalize(basis(d) * vec3<f32>(r * cos(a), r * sin(a), 1.0));
}

fn box_t(n: N, o: vec3<f32>, inv: vec3<f32>, tmax: f32) -> f32 {
  let t0 = (n.lo - o) * inv; let t1 = (n.hi - o) * inv;
  let a = min(t0, t1); let b = max(t0, t1);
  let near = max(max(a.x, a.y), max(a.z, 0.0));
  let far = min(min(b.x, b.y), min(b.z, tmax));
  return select(1e30, near, near <= far);
}
fn tri_t(t: T, o: vec3<f32>, d: vec3<f32>) -> f32 {
  let pv = cross(d, t.e2.xyz); let det = dot(t.e1.xyz, pv);
  if (abs(det) < 1e-12) { return -1.0; }
  let inv = 1.0 / det; let tv = o - t.v0;
  let u = dot(tv, pv) * inv; if (u < 0.0 || u > 1.0) { return -1.0; }
  let qv = cross(tv, t.e1.xyz); let v = dot(d, qv) * inv; if (v < 0.0 || u + v > 1.0) { return -1.0; }
  return dot(t.e2.xyz, qv) * inv;
}
struct Hit { t: f32, i: i32 };
fn closest(o: vec3<f32>, d: vec3<f32>, tmax0: f32) -> Hit {
  var st: array<u32, 48>; var sp = 0; var cur = 0u; var tmax = tmax0; var best = -1;
  let inv = 1.0 / d;
  loop {
    let n = nodes[cur];
    if (n.b > 0u) {
      for (var k = 0u; k < n.b; k++) {
        let t = tri_t(tris[n.a + k], o, d);
        if (t > 0.0005 && t < tmax) { tmax = t; best = i32(n.a + k); }
      }
    } else {
      let l = n.a; let r = n.a + 1u;
      let tl = box_t(nodes[l], o, inv, tmax); let tr = box_t(nodes[r], o, inv, tmax);
      let hl = tl < 1e29; let hr = tr < 1e29;
      if (hl && hr) {
        if (sp < 47) { if (tl < tr) { st[sp] = r; cur = l; } else { st[sp] = l; cur = r; } sp++; continue; }
      } else if (hl) { cur = l; continue; } else if (hr) { cur = r; continue; }
    }
    if (sp == 0) { break; }
    sp--; cur = st[sp];
  }
  return Hit(tmax, best);
}
fn occluded(o: vec3<f32>, d: vec3<f32>, tmax: f32) -> bool {
  var st: array<u32, 48>; var sp = 0; var cur = 0u;
  let inv = 1.0 / d;
  loop {
    let n = nodes[cur];
    if (n.b > 0u) {
      for (var k = 0u; k < n.b; k++) {
        let tr_ = tris[n.a + k]; let t = tri_t(tr_, o, d);
        // (a fitting doesn't stop a shadow ray: e1's w is 0)
        if (t > 0.0005 && t < tmax && tr_.e1.w > 0.5) { return true; }
      }
    } else {
      let l = n.a; let r = n.a + 1u;
      let tl = box_t(nodes[l], o, inv, tmax); let tr = box_t(nodes[r], o, inv, tmax);
      let hl = tl < 1e29; let hr = tr < 1e29;
      if (hl && hr) { if (sp < 47) { st[sp] = r; sp++; cur = l; continue; } }
      else if (hl) { cur = l; continue; } else if (hr) { cur = r; continue; }
    }
    if (sp == 0) { break; }
    sp--; cur = st[sp];
  }
  return false;
}
// one lamp, chosen by what it gives here
fn lamp(o: vec3<f32>, n: vec3<f32>) -> vec3<f32> { return lamp_rest(o, n, false); }
// each lamp's share of the direct light traced apart at this pixel: the rest of it goes with the filtered light
var<private> lamp_share: array<f32, 64>;
// one lamp, chosen by what it gives here; with `rest`, only what is left after each lamp's share
fn lamp_rest(o: vec3<f32>, n: vec3<f32>, rest: bool) -> vec3<f32> {
  let nl = p.misc.z; if (nl == 0u) { return vec3<f32>(0.0); }
  var total = 0.0; var i = 0u;
  loop { if (i >= nl) { break; }
    let v = lights[i].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
    total += select(1.0, 1.0 - lamp_share[i], rest) * dot(lights[i].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
    i++; }
  if (total <= 0.0) { return vec3<f32>(0.0); }
  var pick = rnd() * total; i = 0u;
  loop { if (i >= nl) { break; }
    let v = lights[i].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
    let keep = select(1.0, 1.0 - lamp_share[i], rest);
    let w = keep * dot(lights[i].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
    if (pick < w) {
      if (occluded(o, l, sqrt(r2) - 0.02)) { return vec3<f32>(0.0); }
      return keep * lights[i].col.xyz * max(dot(n, l), 0.0) / r2 * (total / w) * p.scale.x;
    }
    pick -= w;
    i++; }
  return vec3<f32>(0.0);
}
// how much of a lamp, a small sphere, this point sees: two rays, and more only where they differ, on a shadow's soft edge
fn lamp_vis(o: vec3<f32>, c: vec3<f32>) -> f32 {
  var acc = 0.0; var first = 0.0;
  for (var k = 0; k < 8; k++) {
    let t = vec3<f32>(rnd() - 0.5, rnd() - 0.5, rnd() - 0.5) * 2.0 * LAMP_R;
    let v = c + t - o; let d = length(v);
    let seen = select(1.0, 0.0, occluded(o, v / d, d - 0.02));
    acc += seen;
    if (k == 0) { first = seen; }
    if (k == 1 && seen == first) { return seen; }
  }
  return acc / 8.0;
}
fn sun_vis(o: vec3<f32>, n: vec3<f32>) -> f32 {
  let s = p.sun_dir.xyz;
  if (dot(n, s) <= 0.0) { return 0.0; }
  return select(1.0, 0.0, occluded(o, cone(s, p.scale.w), 1000.0));
}
const LAMP_R = 0.025; // m: their radii in the house sidecars, 2-3 cm
// where a ray from a visible point starts: mostly back toward the camera, the one way known to be free. Along the normal alone, a point where a curtain's side or a casing meets a wall started behind the wall, and saw the sun outside
fn start_at(pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
  return pos + normalize(p.cam.xyz - pos) * 0.003 + n * 0.001;
}
// how far a traced pixel's light holds at a surface point: near it, and on the same surface
fn light_weight(q0: vec2<i32>, f: vec2<f32>, n: vec3<f32>, pos: vec3<f32>, dims: vec2<f32>, ss: i32, lw: i32, lh: i32) -> f32 {
  let q = clamp(q0, vec2<i32>(0), vec2<i32>(lw - 1, lh - 1));
  let gq = q * ss + vec2<i32>(ss / 2);
  let dq = textureLoad(g_depth, gq, 0);
  if (dq >= 1.0) { return 0.0; }
  let pq = world_of(gq, dq, dims, p.inv_vp);
  let nq = textureLoad(g_nrm, gq, 0).xyz;
  let bw = (1.0 - abs(f.x - f32(q0.x))) * (1.0 - abs(f.y - f32(q0.y)));
  return bw * pow(max(dot(n, normalize(nq + vec3<f32>(1e-5))), 0.0), 8.0) * exp(-abs(dot(n, pq - pos)) / 0.02);
}
// the sky seen through one window, chosen by what it gives here: most of a room's light comes in through a small part of what it sees, and rays sent at random rarely find it
fn portal(o: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
  let nw = p.misc2.z; if (nw == 0u) { return vec3<f32>(0.0); }
  var total = 0.0; var i = 0u;
  loop { if (i >= nw) { break; }
    let w = wins[i]; let v = w.c.xyz - o; let r2 = max(dot(v, v), 0.05); let l = v * inverseSqrt(r2);
    total += w.c.w * max(dot(n, l), 0.03) * max(abs(dot(l, w.n.xyz)), 0.1) / r2;
    i++; }
  if (total <= 0.0) { return vec3<f32>(0.0); }
  var pick = rnd() * total; i = 0u;
  loop { if (i >= nw) { break; }
    let w = wins[i]; let v = w.c.xyz - o; let r2 = max(dot(v, v), 0.05); let l0 = v * inverseSqrt(r2);
    let wt = w.c.w * max(dot(n, l0), 0.03) * max(abs(dot(l0, w.n.xyz)), 0.1) / r2;
    if (pick < wt || i == nw - 1u) {
      let x = w.c.xyz + w.u.xyz * (2.0 * rnd() - 1.0) + w.v.xyz * (2.0 * rnd() - 1.0);
      let d = x - o; let d2 = dot(d, d); let dl = sqrt(d2); let l = d / dl;
      let cn = dot(n, l); let cw = abs(dot(l, w.n.xyz));
      if (cn <= 0.0 || cw <= 0.0) { return vec3<f32>(0.0); }
      if (occluded(o, l, 1000.0)) { return vec3<f32>(0.0); }
      // (close to the pane the estimate runs away: a surface there sees at most the sky itself)
      let est = sky_at(l) * cn * cw * w.c.w / (d2 * PI) * (total / max(wt, 1e-6));
      let m_ = max(max(est.x, est.y), est.z);
      return est * min(1.0, 4.0 / max(m_, 1e-5));
    }
    pick -= wt; i++; }
  return vec3<f32>(0.0);
}
// does a ray that escaped leave by a window: portal() has counted that light
fn through_window(o: vec3<f32>, d: vec3<f32>) -> bool {
  let nw = p.misc2.z; var i = 0u;
  loop { if (i >= nw) { break; }
    let w = wins[i];
    let dn = dot(d, w.n.xyz);
    if (abs(dn) > 1e-4) {
      let t = dot(w.c.xyz - o, w.n.xyz) / dn;
      if (t > 0.0) {
        let q = o + d * t - w.c.xyz;
        let uu = dot(w.u.xyz, w.u.xyz); let vv = dot(w.v.xyz, w.v.xyz);
        if (abs(dot(q, w.u.xyz)) < uu && abs(dot(q, w.v.xyz)) < vv) { return true; }
      }
    }
    i++; }
  return false;
}
// light arriving at a surface directly
fn direct(o: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
  return p.sun_col.rgb * max(dot(n, p.sun_dir.xyz), 0.0) * sun_vis(o, n) / PI + lamp(o, n) + portal(o, n);
}
// light gathered from the hemisphere over n, by paths (lit units: E / pi)
fn gather(o0: vec3<f32>, n0: vec3<f32>) -> vec3<f32> {
  var o = o0; var n = n0; var tp = vec3<f32>(1.0); var e = vec3<f32>(0.0);
  let bounces = p.misc.x;
  var lc_sl = array<u32, 2>(LC_MASK + 1u, LC_MASK + 1u); var lc_e = array<vec3<f32>, 2>(vec3<f32>(0.0), vec3<f32>(0.0)); var lc_t = lc_e;
  var lc_read = false; var lc_last = vec3<f32>(0.0); var lc_r = 0.0;
  // (the first of these is the first bounce: Blender's limit of 4 is 4 of them. One more ran every room's light a tenth high)
  for (var b = 0u; b < bounces; b++) {
    let d = cosine(n);
    let h = closest(o, d, 1000.0);
    if (b == 0u) { g1_pos = o + d * 1000.0; g1_nrm = vec3<f32>(0.0); }
    if (h.i < 0) { if (!through_window(o, d)) { e += tp * sky_at(d); } break; }
    let t = tris[h.i];
    let m = mats[t.mat];
    var ng = normalize(cross(t.e1.xyz, t.e2.xyz));
    if (dot(ng, d) > 0.0) { ng = -ng; }
    let q = o + d * h.t + ng * 0.002;
    if (b == 0u) { g1_pos = q; g1_nrm = ng; }
    e += tp * m.emissive.rgb;
    tp *= m.albedo.rgb;
    // (the last hit stands in for the bounces left out, up to the four Blender follows: each about as lit as this one, and dimmer by its colour. Stopped short, a sunlit room with few lamps lost a tenth of its light)
    var more = 1.0;
    if (b + 1u == bounces) { let r_ = dot(m.albedo.rgb, vec3<f32>(0.333)); var rk = 1.0; for (var k = bounces; k < 4u; k++) { rk *= r_; more += rk; } }
    if (lc_on() && b + 1u < bounces && b < 2u) {
      let sl = lc_find(lc_jit(q, ng), ng, t.mat);
      lc_sl[b] = sl;
      // (the cache from the first hit that is further than a cell: nearer, in a corner or under a shelf, a cell's average would wash out the shadow. Where it has the light, and but for one path in 32, which goes on and adds to it)
      let lv_ = lc_live(sl);
      if (!lc_read && h.t >= LC_SIZE && b <= 1u && sl <= LC_MASK && lv_.w >= LC_MIN && (lv_.w >= LC_MAX || rnd() > 0.03125)) {
        // (a read one bounce on passes its light back to the hit before, so light builds up over frames with no limit on bounces. A cell stands for this hit and 3 more: from a hit that follows another, 2 more would be right, the same total as Blender's 4 in a room of one colour)
        var v_ = lv_.xyz;
        if (b == 1u) { let r_ = dot(m.albedo.rgb, vec3<f32>(0.333)); let s_ = 1.0 + r_ + r_ * r_; v_ *= s_ / (s_ + r_ * r_ * r_); }
        e += tp * v_; lc_sl[b] = LC_MASK + 1u; lc_read = true; break;
      }
    }
    // (a cell holds all the light reaching its surface, the direct included: whoever reads it needs no shadow rays. Never read from the camera's own hit, so no shadow seen directly is averaged)
    if (b < 2u) { lc_e[b] = e; lc_t[b] = tp; }
    let direct_here = direct(q, ng);
    e += tp * direct_here * more;
    o = q; n = ng;
    if (b + 1u == bounces) { lc_last = tp * direct_here; lc_r = dot(m.albedo.rgb, vec3<f32>(0.333)); }
    // (Russian roulette: a path that has lost most of its light mostly ends here, and the ones that go on count for those that didn't. The same light on average, for fewer rays)
    if (b >= 1u) { let q_ = clamp(max(tp.x, max(tp.y, tp.z)) * 1.5, 0.15, 1.0); if (rnd() > q_) { break; } tp /= q_; }
  }
  if (lc_on()) {
    var est = vec3<f32>(0.0); var rk = 1.0; for (var k = bounces; k < 4u; k++) { rk *= lc_r; }
    for (var k = 0u; k < 2u; k++) {
      // (hit k has k bounces fewer than the first: the next terms of the series)
      if (k > 0u) { rk *= lc_r; est += lc_last * rk; }
      let sl = lc_sl[k]; if (sl > LC_MASK) { continue; }
      let tk = lc_t[k]; if (max(tk.x, max(tk.y, tk.z)) <= 0.001) { continue; }
      lc_add(sl, (e + est - lc_e[k]) / max(tk, vec3<f32>(0.001)));
    }
  }
  return e;
}

@compute @workgroup_size(8, 8) fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let g = px * ss + vec2<i32>(ss / 2);
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(0.0)); textureStore(out_lamp, px, vec4<f32>(0.0)); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 73856093u) ^ (u32(px.y) * 19349663u) ^ (p.misc.y * 83492791u);
  let o = start_at(pos, n);
  // (the cache's first fill: one path from one pixel in each 4x4 block, so the full trace after it can already read the cache)
  if ((u32(p.cam.w) & 16u) != 0u) {
    var h_ = (u32(px.x >> 2u) * 73856093u) ^ (u32(px.y >> 2u) * 19349663u); h_ ^= h_ >> 13u; h_ *= 83492791u; h_ ^= h_ >> 16u;
    if (u32(px.x & 3) == (h_ & 3u) && u32(px.y & 3) == ((h_ >> 2u) & 3u)) { seed ^= 0x9e3779b9u; _ = gather(o, n); }
    return;
  }
  // the lamps that matter here, apart from the filtered light, which would wash out their shadows. Each comes in gradually with its share of the light: a fixed number of lamps per pixel let a lamp drop out all at once, and cut its shadow off in a straight line across a wall
  // (close up, with only the lamp's own size as a limit: a ceiling beside a lamp is really that bright, and a floor on the distance left it dull)
  var ltotal = 0.0; var li = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    ltotal += dot(lights[li].col.xyz, vec3<f32>(0.333)) * max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    li++; }
  var dl_ = vec3<f32>(0.0); li = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    let c_ = max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    let f_ = clamp((dot(lights[li].col.xyz, vec3<f32>(0.333)) * c_ / max(ltotal, 1e-6) - 0.04) / 0.08, 0.0, 1.0);
    lamp_share[li] = f_;
    if (f_ > 0.0) { dl_ += f_ * lights[li].col.xyz * c_ * p.scale.x * lamp_vis(o, lights[li].pos.xyz); }
    li++; }
  // a sheer curtain, a lampshade: part of what lies behind comes through along the view, blurred, tinted by the cloth. (Directional, as in Blender: a lamp off to one side behind a shade does not make it glow)
  var thru_e = vec3<f32>(0.0); // (with the filtered light: from so few rays it is noise)
  let sheer = (textureLoad(g_mat, g, 0).w - 1.0) % 2.0;
  if (sheer > 0.001) {
    dl_ *= 1.0 - sheer;
    let rough_ = textureLoad(g_alb, g, 0).a;
    let vd = normalize(pos - p.cam.xyz);
    var thru = vec3<f32>(0.0);
    for (var k = 0; k < 2; k++) {
      let rd = cone(vd, min(rough_ * rough_, 0.8));
      let hh = closest(pos + vd * 0.012, rd, 1000.0);
      if (hh.i < 0) { thru += sky_at(rd); } else {
        let t_ = tris[hh.i]; let m_ = mats[t_.mat];
        var ng = normalize(cross(t_.e1.xyz, t_.e2.xyz));
        if (dot(ng, rd) > 0.0) { ng = -ng; }
        let q = pos + vd * 0.012 + rd * hh.t + ng * 0.002;
        thru += m_.emissive.rgb + m_.albedo.rgb * direct(q, ng);
      }
    }
    thru *= 0.5;
    thru_e = sheer * thru;
  }
  textureStore(out_lamp, px, vec4<f32>(dl_, gtao(px, g, pos, n, d, dims, w, h, ss)));
  var e = vec3<f32>(0.0);
  let spp = p.dims.w;
  let restir_ = (u32(p.cam.w) & 2u) != 0u;
  var rs = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), f32(spp), vec3<f32>(0.0), 0.0);
  for (var s = 0u; s < spp; s++) {
    if (restir_) {
      e += lamp_rest(o, n, true) + portal(o, n);
      let lg = gather(o, n); let lc = lg * min(1.0, 40.0 / max(max(max(lg.x, lg.y), lg.z), 1e-5));
      let lm = dot(lc, vec3<f32>(0.333)); let wi = lm * PI;
      rs.wsum += wi;
      if (rnd() * rs.wsum < wi) { rs.pos = g1_pos; rs.nrm = g1_nrm; rs.rad = lc; rs.lp = lm; }
      continue;
    }
    let es = lamp_rest(o, n, true) + portal(o, n) + gather(o, n);
    // (only what cannot be real: a path that finds a sunlit patch is rare, valid, and most of a room's warm fill)
    e += es * min(1.0, 40.0 / max(max(max(es.x, es.y), es.z), 1e-5));
  }
  if (restir_) { resv[u32(px.y * w + px.x)] = rs; textureStore(out_irr, px, vec4<f32>(e / f32(spp) * (1.0 - max(sheer, 0.0)) + thru_e, 1.0 - max(sheer, 0.0))); return; }
  e = e / f32(spp) * (1.0 - max(sheer, 0.0)) + thru_e;
  textureStore(out_irr, px, vec4<f32>(e, 0.0));
}

// At the G-buffer's own resolution: the sun, whose edges a filtered picture would turn to steps, and the light on whatever no traced pixel landed on: a thin rail, a door's groove, which came out in dashes where the traced pixels happened to fall.
// a = the sun's visibility; r < 0: the filtered light will do here
@compute @workgroup_size(8, 8) fn trace_fine(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let lw = i32(p.dims.x); let lh = i32(p.dims.y); let w = lw * ss; let h = lh * ss;
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let dims = vec2<f32>(f32(w), f32(h));
  let d = textureLoad(g_depth, px, 0);
  if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(-1.0, 0.0, 0.0, 0.0)); return; }
  let pos = world_of(px, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, px, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  // (a real hash: with a plain product of the coordinates the first random number follows the rows, and a sun edge came out as a comb)
  seed = u32(px.x) + u32(px.y) * 4099u + p.misc.y * 65521u;
  seed = seed * 747796405u + 2891336453u; seed = ((seed >> ((seed >> 28u) + 4u)) ^ seed) * 277803737u; seed = (seed >> 22u) ^ seed;
  let o = start_at(pos, n);
  // (two rays; only where they differ, on a shadow's soft edge, more)
  var vis = sun_vis(o, n);
  if (dot(n, p.sun_dir.xyz) > 0.0) {
    let v2 = sun_vis(o, n);
    if (v2 != vis) {
      var acc = vis + v2;
      for (var k = 0; k < 6; k++) { acc += sun_vis(o, n); }
      vis = acc / 8.0;
    }
  }
  let f = (vec2<f32>(px) + 0.5) / f32(ss) - 0.5;
  let b = vec2<i32>(floor(f));
  var tw = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    tw += light_weight(b + vec2<i32>(i, j), f, n, pos, dims, ss, lw, lh);
  } }
  var e = vec3<f32>(-1.0, 0.0, 0.0);
  if (tw < 0.02) {
    e = vec3<f32>(0.0);
    let spp = p.dims.w;
    for (var s = 0u; s < spp; s++) {
      let es = lamp(o, n) + portal(o, n) + gather(o, n);
      e += es * min(1.0, 40.0 / max(max(max(es.x, es.y), es.z), 1e-5));
    }
    e /= f32(spp);
  }
  textureStore(out_irr, px, vec4<f32>(e, vis));
}

// what a glossy surface sends to the eye from one direction, cosine included: F D G / 4 nv. The target ReSTIR's reflections choose by: each pixel's own roughness and normal are in it, so its smudges stay, where a blur averages them away
fn brdf_nl(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
  let nl = dot(n, l); let nv = max(dot(n, v), 1e-3);
  if (nl <= 0.0) { return vec3<f32>(0.0); }
  let h_ = normalize(v + l); let nh = max(dot(n, h_), 0.0); let vh = max(dot(v, h_), 0.0);
  let al = max(rough * rough, 0.001); let a2 = al * al;
  let dn = nh * nh * (a2 - 1.0) + 1.0; let dd = a2 / (PI * dn * dn);
  let g1v = 2.0 * nv / (nv + sqrt(a2 + (1.0 - a2) * nv * nv));
  let g1l = 2.0 * nl / (nl + sqrt(a2 + (1.0 - a2) * nl * nl));
  let g2 = g1v * g1l / max(g1v + g1l - g1v * g1l, 1e-3);
  return (f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vh, 5.0)) * dd * g2 / (4.0 * nv);
}
fn rs_add(rs: ptr<function, R>, pos_: vec3<f32>, nrm_: vec3<f32>, l: vec3<f32>, wt: vec3<f32>, n: vec3<f32>, v: vec3<f32>, rd: vec3<f32>, rough: f32, f0: vec3<f32>) {
  // (the ray came from the GGX lobe, so what it brings over its probability is its weight, before the fit compose divides by)
  let wi = dot(l * wt * dfg_of(rough, max(dot(n, v), 1e-3), f0), vec3<f32>(0.333));
  if (wi <= 0.0) { return; }
  (*rs).wsum += wi;
  if (rnd() * (*rs).wsum < wi) { (*rs).pos = pos_; (*rs).nrm = nrm_; (*rs).rad = l; (*rs).lp = dot(l * brdf_nl(n, v, rd, rough, f0), vec3<f32>(0.333)); }
}
fn dfg_of(rough: f32, nv: f32, f0: vec3<f32>) -> vec3<f32> {
  let ab = dfg_ab(rough, nv);
  return max(f0 * ab.x + vec3<f32>(ab.y), vec3<f32>(0.005));
}
@compute @workgroup_size(8, 8) fn restir_spec(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let cur_ = textureLoad(irr_done, px, 0); // (the reflections so far: what is no reservoir's, in rgb, and the roughness)
  let off = u32(w * h);
  let r0 = resv[off + u32(px.y * w + px.x)];
  if (r0.m <= 0.0 || cur_.a <= 0.15) { textureStore(out_spec, px, cur_); return; }
  let g = px * ss + vec2<i32>(ss / 2); let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_spec, px, cur_); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  let vv = normalize(p.cam.xyz - pos); let rough = cur_.a;
  let al_ = textureLoad(g_alb, g, 0); let f0 = mix(vec3<f32>(0.04), al_.rgb, textureLoad(g_mat, g, 0).x);
  seed = (u32(px.x) * 73856093u) ^ (u32(px.y) * 2654435761u) ^ 0x51ed270bu;
  let o = start_at(pos, n);
  var sel = r0; var wt = r0.wsum; var mt = r0.m;
  for (var k = 0u; k < RS_K; k++) {
    let a = rnd() * 2.0 * PI; let rr = sqrt(rnd()) * RS_SPEC_RADIUS;
    let q = px + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * rr));
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2); let dq = textureLoad(g_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq_ = textureLoad(g_nrm, gq, 0).xyz; if (dot(nq_, nq_) < 0.25) { continue; }
    let nq = normalize(nq_);
    let pq = world_of(gq, dq, dims, p.inv_vp);
    if (dot(n, nq) < 0.9 || abs(dot(n, pq - pos)) > 0.03) { continue; }
    let rq = resv[off + u32(q.y * w + q.x)];
    if (rq.m <= 0.0) { continue; }
    if (rq.wsum <= 0.0 || rq.lp <= 0.0) { mt += rq.m; continue; }
    let vr = rq.pos - pos; let vq = rq.pos - pq; let lr2 = dot(vr, vr); let lq2 = dot(vq, vq);
    let lr = vr * inverseSqrt(max(lr2, 1e-8));
    let pr = dot(rq.rad * brdf_nl(n, vv, lr, rough, f0), vec3<f32>(0.333));
    if (pr <= 0.0) { mt += rq.m; continue; }
    var jac = 1.0;
    if (dot(rq.nrm, rq.nrm) > 0.25) {
      let sr = abs(dot(rq.nrm, lr)); let sq = abs(dot(rq.nrm, vq)) * inverseSqrt(max(lq2, 1e-8));
      jac = (sr / max(sq, 1e-3)) * (lq2 / max(lr2, 1e-6));
      if (jac > 10.0 || jac < 0.1) { continue; }
    }
    let wn = pr / rq.lp * rq.wsum * jac;
    wt += wn; mt += rq.m;
    if (rnd() * wt < wn) { sel = rq; }
  }
  var ind = vec3<f32>(0.0);
  if (sel.lp > 0.0 && wt > 0.0) {
    let v = sel.pos - pos; let l2 = dot(v, v); let l = v * inverseSqrt(max(l2, 1e-8));
    let fn_ = sel.rad * brdf_nl(n, vv, l, rough, f0); let ps = dot(fn_, vec3<f32>(0.333));
    if (ps > 0.0 && !occluded(o, l, sqrt(l2) - 0.02)) { ind = fn_ / ps * (wt / max(mt, 1.0)) / dfg_of(rough, max(dot(n, vv), 1e-3), f0); }
  }
  textureStore(out_spec, px, vec4<f32>(min(cur_.rgb + ind, vec3<f32>(20.0)), rough));
}
const RS_SPEC_RADIUS = 10.0;

// ReSTIR GI's spatial step: a few neighbours on the same surface offer the bounce they kept, weighed by what it would give here. One shadow ray for the winner
const RS_K = 5u; const RS_RADIUS = 24.0;
@compute @workgroup_size(8, 8) fn restir(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let dir_ = textureLoad(irr_done, px, 0);
  let g = px * ss + vec2<i32>(ss / 2); let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(dir_.rgb, 0.0)); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 2654435761u) ^ (u32(px.y) * 19349663u) ^ 0x9e3779b9u;
  let o = start_at(pos, n);
  let r0 = resv[u32(px.y * w + px.x)];
  var sel = r0; var wt = r0.wsum; var mt = r0.m;
  for (var k = 0u; k < RS_K; k++) {
    let a = rnd() * 2.0 * PI; let rr = sqrt(rnd()) * RS_RADIUS;
    let q = px + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * rr));
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2); let dq = textureLoad(g_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq_ = textureLoad(g_nrm, gq, 0).xyz; if (dot(nq_, nq_) < 0.25) { continue; }
    let nq = normalize(nq_);
    let pq = world_of(gq, dq, dims, p.inv_vp);
    if (dot(n, nq) < 0.9 || abs(dot(n, pq - pos)) > 0.03) { continue; }
    let rq = resv[u32(q.y * w + q.x)];
    if (rq.wsum <= 0.0) { mt += rq.m; continue; }
    let vr = rq.pos - pos; let vq = rq.pos - pq; let lr2 = dot(vr, vr); let lq2 = dot(vq, vq);
    let cr = dot(n, vr) * inverseSqrt(max(lr2, 1e-8)); let cq = dot(nq, vq) * inverseSqrt(max(lq2, 1e-8));
    if (cr <= 0.0 || cq <= 0.001) { continue; }
    // (from one point to the other, the same spot lies at another angle and distance. A sample of sky is a direction: the same either way)
    var jac = 1.0;
    if (dot(rq.nrm, rq.nrm) > 0.25) {
      let sr = abs(dot(rq.nrm, vr)) * inverseSqrt(max(lr2, 1e-8)); let sq = abs(dot(rq.nrm, vq)) * inverseSqrt(max(lq2, 1e-8));
      jac = (sr / max(sq, 1e-3)) * (lq2 / max(lr2, 1e-6));
      if (jac > 10.0 || jac < 0.1) { continue; }
    }
    let wn = cr / cq * rq.wsum * jac;
    wt += wn; mt += rq.m;
    if (rnd() * wt < wn) { sel = rq; }
  }
  var ind = vec3<f32>(0.0);
  if (sel.lp > 0.0) {
    let v = sel.pos - pos; let l2 = dot(v, v); let l = v * inverseSqrt(max(l2, 1e-8)); let c = dot(n, l);
    if (c > 0.0 && !occluded(o, l, sqrt(l2) - 0.02)) { ind = sel.rad / sel.lp * (wt / max(mt, 1.0)) / PI; }
  }
  textureStore(out_irr, px, vec4<f32>(dir_.rgb + ind * dir_.a, 0.0));
}

// GTAO, as games do it: how far the surfaces nearby rise above this point, searched in the depth picture along a few directions. For each side of a direction a share sin^2 of the cosine-weighted hemisphere is closed off. Cheap, and the darkening of corners and contacts stays sharp, where the light's filter smooths it away
const AO_R = 0.5; // m
fn gtao(px: vec2<i32>, g: vec2<i32>, pos: vec3<f32>, n: vec3<f32>, d: f32, dims: vec2<f32>, w: i32, h: i32, ss: i32) -> f32 {
  let wp = max(length(world_of(g + vec2<i32>(ss, 0), d, dims, p.inv_vp) - pos), 1e-4);
  let rpx = clamp(AO_R / wp, 3.0, 40.0);
  let rot = fract(52.9829189 * fract(dot(vec2<f32>(px), vec2<f32>(0.06711056, 0.00583715)))) * PI;
  var occ = 0.0;
  for (var k = 0; k < 2; k++) {
    let a = rot + f32(k) * PI / 2.0; let dir = vec2<f32>(cos(a), sin(a));
    for (var side = 0; side < 2; side++) {
      let sg = select(1.0, -1.0, side == 1); var top = 0.0;
      for (var st = 1; st <= 4; st++) {
        let q = px + vec2<i32>(round(dir * sg * rpx * (f32(st) - 0.5 + 0.5 * fract(rot * 7.3)) / 4.0));
        if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { break; }
        let gq = q * ss + vec2<i32>(ss / 2); let dq = textureLoad(g_depth, gq, 0);
        if (dq >= 1.0) { continue; }
        let v = world_of(gq, dq, dims, p.inv_vp) - pos; let l2 = dot(v, v);
        if (l2 < 1e-6) { continue; }
        let sn = dot(n, v) * inverseSqrt(l2); let fall = max(1.0 - l2 / (AO_R * AO_R), 0.0);
        top = max(top, max(sn - 0.05, 0.0) * fall);
      }
      occ += top * top;
    }
  }
  return clamp(1.0 - occ / 4.0, 0.0, 1.0);
}
// the occlusion, over a few pixels of this surface: its noise goes
fn ao_fine(px: vec2<i32>, pos: vec3<f32>, n: vec3<f32>, dims: vec2<f32>, w: i32, h: i32, ss: i32) -> f32 {
  var acc = 0.0; var wa = 0.0;
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    let q = px + vec2<i32>(i, j);
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2); let dq = textureLoad(g_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let wq = f32(3 - abs(i)) * f32(3 - abs(j)) * pow(max(dot(n, normalize(textureLoad(g_nrm, gq, 0).xyz + vec3<f32>(1e-5))), 0.0), 16.0) * exp(-abs(dot(n, world_of(gq, dq, dims, p.inv_vp) - pos)) / 0.02);
    acc += textureLoad(lamp_done, q, 0).a * wq; wa += wq;
  } }
  return select(textureLoad(lamp_done, px, 0).a, acc / wa, wa > 1e-5);
}
// The lamps' direct light, smoothed along its surface where a shadow's edge passes: a wide soft edge is more than a few rays can resolve, and came out speckled
fn lamp_smooth(px: vec2<i32>, pos: vec3<f32>, n: vec3<f32>, dims: vec2<f32>, w: i32, h: i32, ss: i32) -> vec3<f32> {
  let c = textureLoad(lamp_done, px, 0).rgb;
  let m0 = dot(c, vec3<f32>(0.333)); var lo = m0; var hi = m0;
  for (var k = 0; k < 4; k++) {
    let o = array<vec2<i32>, 4>(vec2<i32>(2, 0), vec2<i32>(-2, 0), vec2<i32>(0, 2), vec2<i32>(0, -2))[k];
    let t = dot(textureLoad(lamp_done, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).rgb, vec3<f32>(0.333));
    lo = min(lo, t); hi = max(hi, t);
  }
  if (hi - lo < 0.1 * hi + 0.001) { return c; }
  var acc = vec3<f32>(0.0); var wa = 0.0;
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    let q = px + vec2<i32>(i, j);
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2);
    let dq = textureLoad(g_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq = textureLoad(g_nrm, gq, 0).xyz;
    let wq = f32(3 - abs(i)) * f32(3 - abs(j)) * pow(max(dot(n, normalize(nq + vec3<f32>(1e-5))), 0.0), 16.0) * exp(-abs(dot(n, world_of(gq, dq, dims, p.inv_vp) - pos)) / 0.02);
    acc += textureLoad(lamp_done, q, 0).rgb * wq; wa += wq;
  } }
  return select(c, acc / wa, wa > 1e-4);
}
// The lamps' own highlights on a glossy surface, before Fresnel: reflections of the room carry no lamps, since a lamp is a point and no ray finds it
fn lamp_glint(pos: vec3<f32>, n: vec3<f32>, o: vec3<f32>, g: vec2<i32>, rough: f32, w: i32, h: i32, ss: i32, e: vec3<f32>) -> vec3<f32> {
  if (rough < 0.001 || p.misc2.w == 8u) { return vec3<f32>(0.0); }
  // relief finer than a pixel widens the highlight (Toksvig)
  var nb = n;
  for (var k = 0; k < 4; k++) {
    let dq = array<vec2<i32>, 4>(vec2<i32>(1, 1), vec2<i32>(-1, 1), vec2<i32>(1, -1), vec2<i32>(-1, -1))[k] * ss; // (over the whole traced pixel: what it covers is what widens the highlight)
    let nq = textureLoad(g_nrm, clamp(g + dq, vec2<i32>(0), vec2<i32>(w * ss - 1, h * ss - 1)), 0).xyz;
    nb += select(n, nq, dot(nq, n) > 0.5);
  }
  nb /= 5.0;
  let nbl = max(length(nb), 0.01); let ns = nb / nbl;
  let a0 = sqrt(max(rough * rough * rough * rough + (1.0 - nbl) / nbl, 0.000004));
  let vdir = normalize(p.cam.xyz - pos); let nv = max(dot(ns, vdir), 0.001);
  // the lamps that show most here, each with its shadow ray and its own Fresnel, at this very angle: the averaged share used for a whole room's light is far too little at a glancing one. Any roughness: on a rough dark surface this broad highlight is most of what it shows
  let mt_ = textureLoad(g_mat, g, 0).x; let alb_ = textureLoad(g_alb, g, 0).rgb; let coat_ = textureLoad(g_nrm, g, 0).a;
  let f0_ = mix(vec3<f32>(0.04), alb_, mt_);
  var cs = array<vec3<f32>, 4>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
  var ls = array<vec3<f32>, 4>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
  var ms = array<f32, 4>(0.0, 0.0, 0.0, 0.0); var ds = array<f32, 4>(0.0, 0.0, 0.0, 0.0); var i = 0u;
  loop { if (i >= p.misc.z) { break; }
    let v = lights[i].pos.xyz - pos; let r2 = max(dot(v, v), LAMP_R * LAMP_R); let dl = sqrt(r2); let l = v / dl; // (the lamp's own size as the limit: 20 cm dulled a highlight on anything nearer, inside a shade, a ceiling round a fitting)
    let ndl = dot(ns, l);
    if (ndl > 0.0) {
      // a lamp is a small sphere: it widens the highlight, and spreads the same light (Karis)
      let a = min(a0 + LAMP_R / (2.0 * dl), 1.0); let keep = (a0 / a) * (a0 / a);
      let h_ = normalize(l + vdir); let ndh = max(dot(ns, h_), 0.0);
      let dn = ndh * ndh * (a * a - 1.0) + 1.0;
      let dv = keep * a * a / (PI * dn * dn + 1e-6) * 0.25 / max(mix(nv, 1.0, a * 0.5) * mix(ndl, 1.0, a * 0.5), 0.05);
      let fc_ = pow(1.0 - max(dot(h_, vdir), 0.0), 5.0);
      let fh = f0_ + (vec3<f32>(1.0) - f0_) * fc_ + vec3<f32>(coat_ * (0.04 + 0.96 * fc_));
      // (E / pi is what lamp() adds to the light; the sun's highlight has the same form)
      let c = lights[i].col.xyz * PI / r2 * p.scale.x * ndl * min(dv, 200.0) * fh;
      let m_ = dot(c, vec3<f32>(0.333));
      // kept in order, the brightest first
      var k = 3;
      if (m_ > ms[3]) {
        loop { if (k == 0 || m_ <= ms[k - 1]) { break; } ms[k] = ms[k - 1]; cs[k] = cs[k - 1]; ls[k] = ls[k - 1]; ds[k] = ds[k - 1]; k--; }
        ms[k] = m_; cs[k] = c; ls[k] = l; ds[k] = dl;
      }
    }
    i++; }
  var out = vec3<f32>(0.0);
  for (var k = 0; k < 4; k++) { if (ds[k] > 0.0 && ms[k] > 0.002 && !occluded(o, ls[k], ds[k] - 0.02)) { out += cs[k]; } }
  return min(out, vec3<f32>(50.0));
}
@compute @workgroup_size(8, 8) fn trace_spec(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let g = px * ss + vec2<i32>(ss / 2);
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_spec, px, vec4<f32>(0.0)); textureStore(out_glint, px, vec4<f32>(0.0)); textureStore(out_lamp, px, vec4<f32>(0.0)); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 2654435761u) ^ (u32(px.y) * 40503u) ^ (p.misc.y * 1103515245u);
  let o = start_at(pos, n);
  let e = textureLoad(irr_done, px, 0).rgb + textureLoad(lamp_done, px, 0).rgb;
  // what a glossy surface reflects
  let a = textureLoad(g_alb, g, 0);
  let rough = a.a;
  var sp = vec3<f32>(0.0); var rs_s = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0);
  let coat = textureLoad(g_nrm, g, 0).a;
  if (p.misc2.y > 0u) {
    let v = normalize(pos - p.cam.xyz);
    let r0 = reflect(v, n);
    // (a flat mirror, alb's a 0: what it shows was traced already, as a picture from the camera mirrored in it, with textures and its own light)
    let is_mirror = rough < 0.001;
    // (metal shows nothing but its reflections: full mode gives it one more ray)
    let rays = select(select(p.misc2.y, 4u, p.misc2.y >= 3u && textureLoad(g_mat, g, 0).x >= 0.5), 0u, is_mirror);
    let use_rs = (u32(p.cam.w) & 4u) != 0u && rough > 0.15 && !is_mirror;
    rs_s.m = f32(rays);
    let f0_rs = mix(vec3<f32>(0.04), a.rgb, textureLoad(g_mat, g, 0).x);
    // (relief finer than the traced pixel widens the reflection, Kaplanyan's way: from how far the normals around it spread)
    var nsp = n;
    for (var k = 0; k < 4; k++) {
      let dq = array<vec2<i32>, 4>(vec2<i32>(1, 1), vec2<i32>(-1, 1), vec2<i32>(1, -1), vec2<i32>(-1, -1))[k] * max(ss / 2, 1);
      let nq = textureLoad(g_nrm, clamp(g + dq, vec2<i32>(0), vec2<i32>(w * ss - 1, h * ss - 1)), 0).xyz;
      nsp += select(n, nq, dot(nq, n) > 0.5);
    }
    let nspl = length(nsp) / 5.0;
    let spec_w = select(min(sqrt(max((1.0 - nspl) / max(nspl, 0.01), 0.0)), 0.5), 0.0, is_mirror);
    // (at one sample a pixel that picture has jagged frames and a comb where wall texture beats with the pixels: a tent over the pixel takes them out, as the sub-pixels do for the room)
    if (is_mirror) {
      for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
        sp += textureLoad(m_lit, clamp(px + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).rgb * f32((2 - abs(i)) * (2 - abs(j))) / 16.0;
      } }
    }
    for (var s = 0u; s < rays; s++) {
      // the rough lobe as it really is (GGX, visible normals, Heitz 2018), each ray weighted against the share the composition step will multiply by. A narrow cone left out most of what a rough surface reflects: dark blades and metal doors
      var rd = r0; var wt_ = vec3<f32>(1.0);
      let al_ = min(rough * rough + spec_w, 1.0);
      if (al_ > 0.001) {
        let vl = -v; let bs_ = basis(n);
        let ve = vec3<f32>(dot(vl, bs_[0]), dot(vl, bs_[1]), dot(vl, bs_[2]));
        let vh = normalize(vec3<f32>(al_ * ve.x, al_ * ve.y, ve.z));
        let lsq = vh.x * vh.x + vh.y * vh.y;
        let t1_ = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-vh.y, vh.x, 0.0) * inverseSqrt(max(lsq, 1e-9)), lsq > 1e-9);
        let t2_ = cross(vh, t1_);
        let u_ = rnd(); let r_ = sqrt(u_); let ph = 2.0 * PI * rnd();
        let t1s = r_ * cos(ph); let sv = 0.5 * (1.0 + vh.z);
        let t2s = (1.0 - sv) * sqrt(max(1.0 - t1s * t1s, 0.0)) + sv * r_ * sin(ph);
        let nh = t1s * t1_ + t2s * t2_ + sqrt(max(0.0, 1.0 - t1s * t1s - t2s * t2s)) * vh;
        let ne = normalize(vec3<f32>(al_ * nh.x, al_ * nh.y, max(0.0, nh.z)));
        let h_w = bs_ * ne;
        rd = reflect(v, h_w);
        let nl = dot(rd, n);
        if (nl <= 0.0) { continue; }
        // F * G2 / G1, over what compose multiplies by
        let vdh = max(dot(vl, h_w), 0.0); let nv_ = max(dot(n, vl), 1e-3);
        let f0_ = mix(vec3<f32>(0.04), a.rgb, textureLoad(g_mat, g, 0).x);
        let fh = f0_ + (vec3<f32>(1.0) - f0_) * pow(1.0 - vdh, 5.0);
        let a2 = al_ * al_;
        let g1v = 2.0 * nv_ / (nv_ + sqrt(a2 + (1.0 - a2) * nv_ * nv_));
        let g1l = 2.0 * nl / (nl + sqrt(a2 + (1.0 - a2) * nl * nl));
        let g2g1 = g1l / max(g1v + g1l - g1v * g1l, 1e-3) ;
        let ab_ = dfg_ab(rough, nv_);
        wt_ = fh * g2g1 / max(f0_ * ab_.x + vec3<f32>(ab_.y), vec3<f32>(0.005));
      }
      if (dot(rd, n) <= 0.0) { rd = r0; }
      let hh = closest(o, rd, 1000.0);
      if (hh.i < 0) {
        var l = sky_at(rd);
        // (the sun only in a truly sharp reflection: on anything rough its highlight is already counted, and a ray that happened on it was a firefly)
        if (rough < 0.03 && dot(rd, p.sun_dir.xyz) > cos(p.scale.w)) { l += p.sun_col.rgb * (1.0 / (2.0 * PI * (1.0 - cos(p.scale.w)))); }
        if (use_rs) { rs_add(&rs_s, o + rd * 1000.0, vec3<f32>(0.0), l, wt_, n, -v, rd, rough, f0_rs); continue; }
        sp += l * wt_;
      } else {
        let t = tris[hh.i]; let m = mats[t.mat];
        var ng = normalize(cross(t.e1.xyz, t.e2.xyz));
        if (dot(ng, rd) > 0.0) { ng = -ng; }
        let q = o + rd * hh.t + ng * 0.002;
        // (the light it sees there, without the noise of more random choices: the sun, and the clean light of this pixel's own surroundings for the rest)
        // (a rough reflection, which the filter smooths: the light where the ray landed, by one path. In a shaded door frame this pixel's own light made what it reflects far too dark)
        // (by one sample of the lamps and windows, the same at any roughness: a limit here drew the jumps in a material's roughness across a floor)
        // (the cache where the reflection's spread at the hit is wider than a cell, by SHaRC's estimate of the cone: a sharper one would show the cell. It holds the sun and lamps too, so no shadow rays. A real branch below: select() runs both sides)
        let a2_ = rough * rough * rough * rough; let spread = 2.0 * hh.t * sqrt(0.5 * a2_ / max(1.0 - a2_, 1e-3));
        var sl = LC_MASK + 1u;
        let on_metal = textureLoad(g_mat, g, 0).x >= 0.5;
        if (lc_on() && rough > 0.15 && hh.t >= LC_SIZE && spread >= LC_SIZE) { let jq = lc_jit(q, ng); if (on_metal) { sl = lc_find(jq, ng, t.mat); } else { sl = lc_look(jq, ng, t.mat); } }
        var lh_ = vec3<f32>(0.0);
        if (sl <= LC_MASK && lc_out[sl].w >= LC_MIN) {
          lh_ = m.emissive.rgb + m.albedo.rgb * lc_out[sl].xyz;
        } else {
          // (the light it sees there, without the noise of more random choices: the sun, and the clean light of this pixel's own surroundings for the rest)
          // (a rough reflection, which the filter smooths: the light where the ray landed, by one path. In a shaded door frame this pixel's own light made what it reflects far too dark)
          // (by one sample of the lamps and windows, the same at any roughness: a limit here drew the jumps in a material's roughness across a floor)
          var e_hit = e;
          // (metal in any mode: with one ray too it shows nothing but this, and without the path a fridge washed out and lost its smudges)
          if (rough > 0.15 && (p.misc2.y > 1u || textureLoad(g_mat, g, 0).x >= 0.5)) {
            // (on metal, a real path from where it landed: all it shows is what it reflects, and this pixel's own light says nothing about a sunny wall across the room. A dark door reflected darkness. Too dear for the rest)
            var eh = lamp(q, ng) + portal(q, ng);
            // (what it finds here fills the cell, or a surface only reflections reach would never have one)
            if (on_metal) { eh += gather(q, ng); if (sl <= LC_MASK) { lc_add(sl, p.sun_col.rgb * max(dot(ng, p.sun_dir.xyz), 0.0) * sun_vis(q, ng) / PI + eh); } }
            e_hit = select(0.5 * e, vec3<f32>(0.0), on_metal) + eh * min(1.0, 8.0 / max(max(max(eh.x, eh.y), eh.z), 1e-5));
          }
          lh_ = m.emissive.rgb + m.albedo.rgb * (p.sun_col.rgb * max(dot(ng, p.sun_dir.xyz), 0.0) * sun_vis(q, ng) / PI + e_hit);
        }
        if (use_rs) { rs_add(&rs_s, q, ng, lh_, wt_, n, -v, rd, rough, f0_rs); continue; }
        sp += lh_ * wt_;
      }
    }
    if (rays > 0u) { sp /= f32(rays); }
  }
  textureStore(out_lamp, px, vec4<f32>(lamp_smooth(px, pos, n, dims, w, h, ss), ao_fine(px, pos, n, dims, w, h, ss)));
  // (not through the reflections' filter: it smeared the shadow each louvre blade casts on the next. Except where roughness comes from a map: its jumps drew hard shapes across a door and blotches on a floor)
  var gl_ = lamp_glint(pos, n, o, g, rough, w, h, ss, e);
  if (rough > 0.08 && textureLoad(g_mat, g, 0).w >= 2.9 && coat < 0.25 && p.misc2.y > 0u) {
    let vl_ = normalize(p.cam.xyz - pos); let nv_g = max(dot(n, vl_), 1e-3);
    let abg = dfg_ab(rough, nv_g);
    let f0g = mix(vec3<f32>(0.04), a.rgb, textureLoad(g_mat, g, 0).x);
    sp += min(gl_ / max(f0g * abg.x + vec3<f32>(abg.y), vec3<f32>(0.005)), vec3<f32>(20.0)); gl_ = vec3<f32>(0.0);
  }
  textureStore(out_glint, px, vec4<f32>(gl_, 0.0));
  if ((u32(p.cam.w) & 4u) != 0u) { resv[u32(w * h) + u32(px.y * w + px.x)] = rs_s; }
  textureStore(out_spec, px, vec4<f32>(min(sp, vec3<f32>(20.0)), rough));
}
