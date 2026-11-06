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
@group(0) @binding(32) var out_sun: texture_storage_2d<rgba16float, write>; // (the sun's visibility at each traced pixel: a video keeps a history of it, as of the lamps)
@group(0) @binding(16) var lamp_done: texture_2d<f32>;
@group(0) @binding(17) var g_mat: texture_2d<f32>;
struct W { c: vec4<f32>, u: vec4<f32>, v: vec4<f32>, n: vec4<f32> };
@group(0) @binding(11) var<storage, read> wins: array<W>;
// ReSTIR GI, within one frame: each pixel keeps one of its first bounces, chosen by how much light it brought (Ouyang et al. 2021)
struct R { pos: vec3<f32>, wsum: f32, nrm: vec3<f32>, m: f32, rad: vec3<f32>, lp: f32 };
@group(0) @binding(18) var<storage, read_write> resv: array<R>;
// the light cache: bounced light arriving at a surface. Not its colour, which whoever reads it has; not the sun, lamps or windows, whose sharp shadows stay traced; one cell per 20 cm, side of a surface and part, so nothing leaks through a wall. Filled by the paths themselves, at every hit but the last: what a path gathers after a hit, over what reached that hit, is a sample of this. From a later hit it has fewer bounces left, so what those would have brought is estimated from the path's last light and the colour there, to keep Blender's count: no cell reads another while filling, or it would run on without end
@group(0) @binding(22) var hist_n: texture_2d<f32>;
// (what each coarse cell has seen by direction: for 8 parts of its hemisphere, the brightness brought back and how many paths brought it. Rays aim where the light is)
// (what the trace found at each traced pixel: x the sun's visibility; y, z, w six of the lamps traced apart, 16 bits each, the lamp's index over its visibility in 255ths. 0xffff: none)
@group(0) @binding(27) var<storage, read_write> vis_sh: array<vec4<u32>>;
fn vis_lamp(e: vec4<u32>, li: u32) -> f32 {
  for (var k = 0u; k < 6u; k++) {
    let wd = select(select(e.w, e.z, k < 4u), e.y, k < 2u); let hv = (wd >> (16u * (k & 1u))) & 0xffffu;
    if (hv != 0xffffu && (hv >> 8u) == li) { return f32(hv & 255u) / 255.0; }
  }
  return -1.0;
}
@group(0) @binding(26) var<storage, read_write> lc_guide: array<array<atomic<u32>, 16>>;
var<private> g_use = false; var<private> g_dir = vec3<f32>(0.0); var<private> g_wt = 1.0;
// (a video's reservoirs, last frame's and this frame's, turn about, and where a point was last frame)
struct PV { vp: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(23) var<uniform> pv: PV;
@group(0) @binding(24) var<storage, read> resv_a: array<R>;
@group(0) @binding(25) var<storage, read_write> resv_b: array<R>;
// (has this surface a settled light from earlier frames? Then it needs nothing new: the scene holds still)
fn hist_settled(pos: vec3<f32>, n: vec3<f32>, w: i32, h: i32) -> bool {
  let ip = rs_prev(pos, w, h); if (ip < 0) { return false; }
  let hn = textureLoad(hist_n, vec2<i32>(ip % w, ip / w), 0);
  let ex = length(pos - pv.eye.xyz);
  return hn.r >= 3.0 && abs(hn.g - ex) <= 0.02 + 0.005 * ex && abs(hn.b - n.x) + abs(hn.a - n.y) <= 0.3;
}
// (is what is around it settled? Then this is an edge or a fine detail that misses its history by a hair each frame, not a surface coming into view. It is too small for a cell to show as a patch, and tracing it every frame was most of a slow move's cost)
fn hist_around(pos: vec3<f32>, w: i32, h: i32) -> bool {
  let ip = rs_prev(pos, w, h); if (ip < 0) { return false; }
  let c = vec2<i32>(ip % w, ip / w); var nb = 0;
  for (var k = 0; k < 4; k++) {
    var o4 = array<vec2<i32>, 4>(vec2<i32>(3, 0), vec2<i32>(-3, 0), vec2<i32>(0, 3), vec2<i32>(0, -3));
    let q = clamp(c + o4[k], vec2<i32>(0), vec2<i32>(w - 1, h - 1));
    if (textureLoad(hist_n, q, 0).r >= 3.0) { nb++; }
  }
  return nb >= 3;
}
fn rs_time() -> bool { return (u32(p.cam.w) & 128u) != 0u; }
fn rs_prev(pos: vec3<f32>, w: i32, h: i32) -> i32 {
  let c = pv.vp * vec4<f32>(pos, 1.0); if (c.w <= 0.0) { return -1; }
  let u = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
  if (any(u < vec2<f32>(0.0)) || any(u >= vec2<f32>(1.0))) { return -1; }
  let q = clamp(vec2<i32>(floor(u * vec2<f32>(f32(w), f32(h)) - vec2<f32>(sub_off()))), vec2<i32>(0), vec2<i32>(w - 1, h - 1)); return q.y * w + q.x;
}
// (where a traced pixel's own surface sample lies, past its centre, in its pixels: it takes the one at ss/2 of its ss x ss, a quarter pixel off at 2x2. Positions come from that sample, so the history's pixels stand there too, and a read of it must allow for it, or the light creeps a quarter pixel each frame)
fn sub_off() -> f32 { let ss = f32(p.dims.z); return (floor(ss * 0.5) + 0.5) / ss - 0.5; }
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
  // (and the size: a 10 cm and a 40 cm cell with the same integer place shared a slot, and a read took another room's light)
  var h = u32(c.x) * 73856093u ^ u32(c.y) * 19349663u ^ u32(c.z) * 83492791u ^ part * 2654435761u ^ side * 3266489917u ^ u32(round(size * 1000.0)) * 2971215073u;
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
// ---- a video's light from the cache: what reaches a surface the camera sees, by place and side, apart from the cells paths read. Filled from the first frame's clean picture, so most of a later frame reads it and traces nothing
const LCV_FINE = 0.1; const LCV_COARSE = 0.4; const LCV_PART = 16384u; const LCV_TRUST_FINE = 48.0; const LCV_TRUST_COARSE = 320.0;
fn lcv_add(q: vec3<f32>, n: vec3<f32>, l: vec3<f32>) { lcv_add_p(q, n, l, LCV_PART); }
// a first direction by the cell's bins: a cosine-weighted direction is a point in a square, and the square has 4 x 2 parts. wt: over the odds, so the total stays the same. Returns the part
const GUIDE_MIN = 32u;
fn guide_pick(sl: u32, n: vec3<f32>) -> u32 {
  g_use = false; g_wt = 1.0;
  if (sl > LC_MASK) { return 8u; }
  var m = array<f32, 8>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); var tot = 0.0; var cnt = 0u;
  for (var i = 0u; i < 8u; i++) { let c = atomicLoad(&lc_guide[sl][8u + i]); cnt += c; if (c > 0u) { m[i] = f32(atomicLoad(&lc_guide[sl][i])) / (f32(c) * LC_FIX); tot += m[i]; } }
  if (cnt < GUIDE_MIN || tot <= 0.0) { return 8u; }
  // (half by what was seen, half evenly: a direction that has shown little may still hold something)
  var r = rnd(); var pick = 7u; var pr = 0.0;
  for (var i = 0u; i < 8u; i++) { pr = 0.5 / 8.0 + 0.5 * m[i] / tot; if (r < pr) { pick = i; break; } r -= pr; }
  pr = 0.5 / 8.0 + 0.5 * m[pick] / tot;
  let u1 = (f32(pick & 1u) + rnd()) * 0.5; let u2 = (f32(pick >> 1u) + rnd()) * 0.25;
  let b_ = basis(n); let ra = sqrt(u1); let ph = 2.0 * PI * u2;
  g_dir = b_[0] * (ra * cos(ph)) + b_[1] * (ra * sin(ph)) + n * sqrt(max(1.0 - u1, 0.0));
  g_wt = 1.0 / (8.0 * pr); g_use = true;
  return pick;
}
// which part an unguided direction fell in, and what it brought back
fn guide_add(sl: u32, n: vec3<f32>, d: vec3<f32>, l: vec3<f32>) {
  if (sl > LC_MASK) { return; }
  let b_ = basis(n); let z = dot(d, n); let u1 = clamp(1.0 - z * z, 0.0, 0.9999);
  var a = atan2(dot(d, b_[1]), dot(d, b_[0])) / (2.0 * PI); if (a < 0.0) { a += 1.0; }
  let i = min(u32(u1 * 2.0), 1u) + min(u32(a * 4.0), 3u) * 2u;
  atomicAdd(&lc_guide[sl][i], u32(min(dot(l, vec3<f32>(0.333)), 64.0) * LC_FIX)); atomicAdd(&lc_guide[sl][8u + i], 1u);
}
fn lcv_add_coarse(q: vec3<f32>, n: vec3<f32>, l: vec3<f32>) {
  let sl = lc_find_k(lc_key_s(q, n, LCV_PART, LCV_COARSE));
  if (sl <= LC_MASK) { lc_add(sl, l); }
}
fn lcv_add_p(q: vec3<f32>, n: vec3<f32>, l: vec3<f32>, part: u32) {
  for (var k = 0; k < 2; k++) {
    let sl = lc_find_k(lc_key_s(q, n, part, select(LCV_FINE, LCV_COARSE, k == 1)));
    if (sl <= LC_MASK) { lc_add(sl, l); }
  }
}
// the four cells round the point, on its surface, each as far as it has filled
fn lcv_at(q: vec3<f32>, n: vec3<f32>, size: f32, part: u32) -> vec4<f32> {
  let an = abs(n); var ax = 2u; if (an.x >= an.y && an.x >= an.z) { ax = 0u; } else if (an.y >= an.z) { ax = 1u; }
  let au = (ax + 1u) % 3u; let av = (ax + 2u) % 3u;
  let fu = q[au] / size - 0.5; let fv = q[av] / size - 0.5;
  let bu = floor(fu); let bv = floor(fv); let tu_ = fu - bu; let tv_ = fv - bv;
  var acc = vec3<f32>(0.0); var sum = 0.0; var lo = 1e9; var hi = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    var c = q; c[au] = (bu + f32(i) + 0.5) * size; c[av] = (bv + f32(j) + 0.5) * size;
    let w = select(1.0 - tu_, tu_, i == 1) * select(1.0 - tv_, tv_, j == 1);
    let sl = lc_look_k(lc_key_s(c, n, part, size));
    if (sl <= LC_MASK && lc_out[sl].w >= 1.0) { let tr = w * min(lc_out[sl].w / select(LCV_TRUST_COARSE, LCV_TRUST_FINE, size < 0.2), 1.0); acc += lc_out[sl].xyz * tr; sum += tr; let lm = dot(lc_out[sl].xyz, vec3<f32>(0.333)); lo = min(lo, lm); hi = max(hi, lm); }
  } }
  lcv_spread = select(0.0, (hi - lo) / max(hi, 1e-4), hi > 0.0);
  return vec4<f32>(acc, sum);
}
// rgb: the light, a: how much of it the cells could answer for
fn lcv_read(q: vec3<f32>, n: vec3<f32>) -> vec4<f32> { return lcv_read_p(q, n, LCV_PART); }
var<private> lcv_spread = 0.0;
fn lcv_read_p(q: vec3<f32>, n: vec3<f32>, part: u32) -> vec4<f32> {
  let a = lcv_at(q, n, LCV_FINE, part);
  if (a.w >= 0.999) { return vec4<f32>(a.xyz, 1.0); }
  let c = lcv_at(q, n, LCV_COARSE, part);
  if (c.w > 0.0) {
    // (a coarse cell is 40 cm: where the light changes within it -- its neighbours differ, or the finer cells here disagree with it -- its average is a patch of the wrong light. It answers for less there, and the pixel traces)
    var u = lcv_spread;
    if (a.w > 0.05) { let fm = dot(a.xyz / a.w, vec3<f32>(0.333)); let cm = dot(c.xyz / c.w, vec3<f32>(0.333)); u = max(u, abs(fm - cm) / max(max(fm, cm), 1e-4)); }
    let k = 1.0 - LCV_DOUBT * smoothstep(0.2, 0.5, u);
    return vec4<f32>(a.xyz + c.xyz / c.w * (1.0 - a.w), min(a.w + c.w * k, 1.0));
  }
  return vec4<f32>(a.xyz, a.w);
}
const LCV_DOUBT = 1.0;
// ---- the reflection cache: at each 40 cm cell of a rough surface, what arrives from each way over its half-sphere, as a small image of directions (8x8, folded flat octahedrally, as games' radiance probes are) with coarser copies for rougher reads; and at 10 cm, how bright a spot is against its 40 cm cell, which scales what the map says. Not the sun or lamps themselves: their highlights are drawn apart
@group(0) @binding(28) var<storage, read_write> m_idx: array<atomic<u32>>; // (each table slot's map, plus one: 0 for none; the last entry counts maps given out)
@group(0) @binding(29) var<storage, read_write> m_acc: array<array<atomic<u32>, 256>>; // (64 directions: r, g, b, samples in 16ths)
@group(0) @binding(30) var<storage, read_write> m_out: array<array<vec4<f32>, 85>>; // (8x8, 4x4, 2x2, 1x1: the light and its samples)
const M_LV = 0u; const M_SIZE = 0.4;
const M_NONE = 0xffffffffu;
const M_MAPS = 16384u; const M_PART = 16600u; const M_DC = 16601u; const M_TRUST = 24.0;
// (the light that came from a path's first direction, as a direction map wants it: the sky itself where it left by a window, which the path leaves to the window sample; else what the path brought, without the guide's weight)
var<private> g1_esc = false;
fn g1_l(lg: vec3<f32>, w_used: f32, d: vec3<f32>) -> vec3<f32> {
  if (g1_esc) { return sky_at(d); }
  return lg / max(w_used, 1e-3);
}
fn m_on() -> bool { return lc_on() && (u32(p.cam.w) & 2048u) != 0u; }
// (the cell's own frame, by the side it is keyed on: the same for whoever writes or reads it)
fn m_frame(ng: vec3<f32>) -> mat3x3<f32> {
  let an = abs(ng); var ax = 2u; if (an.x >= an.y && an.x >= an.z) { ax = 0u; } else if (an.y >= an.z) { ax = 1u; }
  var z = vec3<f32>(0.0); z[ax] = select(1.0, -1.0, ng[ax] < 0.0);
  var x = vec3<f32>(0.0); x[(ax + 1u) % 3u] = 1.0;
  return mat3x3<f32>(x, cross(z, x), z);
}
// (a direction on the half-sphere to the unit square: folded octahedrally, so each texel covers about as much of it)
fn m_uv(d: vec3<f32>, fr: mat3x3<f32>) -> vec2<f32> {
  let l = vec3<f32>(dot(d, fr[0]), dot(d, fr[1]), max(dot(d, fr[2]), 0.0));
  let s = abs(l.x) + abs(l.y) + l.z; let pp = l.xy / max(s, 1e-6);
  return clamp(vec2<f32>(pp.x + pp.y, pp.x - pp.y) * 0.5 + vec2<f32>(0.5), vec2<f32>(0.0), vec2<f32>(0.9999));
}
fn m_slot(q: vec3<f32>, ng: vec3<f32>) -> u32 { return lc_find_k(lc_key_s(q, ng, M_PART, M_SIZE)); }
// one sample of the light arriving from d: into the map of q's cell, if it has one (or may take one), and into the cells of brightness, from paths only
fn m_sample(q: vec3<f32>, ng: vec3<f32>, d: vec3<f32>, l: vec3<f32>, alloc: bool, dc: bool) {
  let lc = clamp(l, vec3<f32>(0.0), vec3<f32>(64.0));
  if (dc) { for (var k = 0u; k < 2u; k++) { let sl = lc_find_k(lc_key_s(q, ng, M_DC, select(LCV_FINE, M_SIZE, k == 1u))); if (sl <= LC_MASK) { lc_add(sl, lc); } } }
  let sl = m_slot(q, ng); if (sl > LC_MASK) { return; }
  var mi = atomicLoad(&m_idx[sl]);
  // (the slot is claimed first, and only its winner takes a number: every thread that saw it empty took one before, and a new floor's first frame used hundreds of the maps. M_NONE: claimed, or none left)
  if (mi == 0u && alloc) {
    let r = atomicCompareExchangeWeak(&m_idx[sl], 0u, M_NONE);
    if (r.exchanged) {
      let nm = atomicAdd(&m_idx[LC_MASK + 1u], 1u);
      if (nm < M_MAPS) { atomicStore(&m_idx[sl], nm + 1u); mi = nm + 1u; }
    }
  }
  if (mi == 0u || mi > M_MAPS) { return; }
  let m = mi - 1u;
  if (m_out[m][84].w >= LC_MAX * 16.0) { return; }
  // (spread over the four nearest directions, as a bilinear read will take it back)
  let f = m_uv(d, m_frame(ng)) * 8.0 - vec2<f32>(0.5); let b = floor(f); let t = f - b;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    let c = clamp(vec2<i32>(b) + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(7));
    let w = select(1.0 - t.x, t.x, i == 1) * select(1.0 - t.y, t.y, j == 1); let wq = u32(round(w * 16.0)); if (wq == 0u) { continue; }
    let k = u32(c.y * 8 + c.x) * 4u; let v = lc * (f32(wq) / 16.0) * LC_FIX;
    atomicAdd(&m_acc[m][k], u32(v.x)); atomicAdd(&m_acc[m][k + 1u], u32(v.y)); atomicAdd(&m_acc[m][k + 2u], u32(v.z)); atomicAdd(&m_acc[m][k + 3u], wq);
  } }
}
fn m_resolve(m: u32) {
  for (var t = 0u; t < 64u; t++) { let c = atomicLoad(&m_acc[m][t * 4u + 3u]); if (c > 0u) { m_out[m][t] = vec4<f32>(f32(atomicLoad(&m_acc[m][t * 4u])), f32(atomicLoad(&m_acc[m][t * 4u + 1u])), f32(atomicLoad(&m_acc[m][t * 4u + 2u])), 0.0) / (LC_FIX * f32(c) / 16.0) + vec4<f32>(0.0, 0.0, 0.0, f32(c)); } }
  // (each coarser copy: its four finer texels, by how many samples each had)
  var src = 0u; var dst = 64u; var r = 4u;
  for (var lv = 0u; lv < 3u; lv++) {
    for (var y = 0u; y < r; y++) { for (var x = 0u; x < r; x++) {
      var a = vec3<f32>(0.0); var c = 0.0;
      for (var k = 0u; k < 4u; k++) { let s = m_out[m][src + (y * 2u + (k >> 1u)) * r * 2u + x * 2u + (k & 1u)]; a += s.xyz * s.w; c += s.w; }
      m_out[m][dst + y * r + x] = vec4<f32>(a / max(c, 1e-6), c);
    } }
    src = dst; dst += r * r; r = r / 2u;
  }
}
// one level of a map along uv: the four nearest texels by how near, and only those that have samples
fn m_tap(m: u32, uv: vec2<f32>, lv: u32) -> vec4<f32> {
  var off = array<u32, 4>(0u, 64u, 80u, 84u); let res = i32(8u >> lv);
  let f = uv * f32(res) - vec2<f32>(0.5); let b = floor(f); let t = f - b;
  var a = vec3<f32>(0.0); var wsum = 0.0; var cnt = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    let c = clamp(vec2<i32>(b) + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(res - 1));
    let s = m_out[m][off[lv] + u32(c.y * res + c.x)];
    let w = select(1.0 - t.x, t.x, i == 1) * select(1.0 - t.y, t.y, j == 1) * select(0.0, 1.0, s.w > 0.0);
    a += s.xyz * w; wsum += w; cnt += s.w * w;
  } }
  return vec4<f32>(a / max(wsum, 1e-6), select(0.0, cnt / max(wsum, 1e-6) / 16.0, wsum > 1e-6));
}
// what a rough reflection at q shows, seen from v: four directions from its lobe, weighted as a traced ray would be, each read from the 40 cm maps around it, and scaled by how bright this spot is against its cell
fn m_read(q: vec3<f32>, n: vec3<f32>, v: vec3<f32>, rough: f32, f0: vec3<f32>, metal: bool) -> vec4<f32> {
  let fr = m_frame(n);
  let an = abs(n); var ax = 2u; if (an.x >= an.y && an.x >= an.z) { ax = 0u; } else if (an.y >= an.z) { ax = 1u; }
  let au = (ax + 1u) % 3u; let av = (ax + 2u) % 3u; let size = M_SIZE;
  let fu = q[au] / size - 0.5; let fv = q[av] / size - 0.5; let bu = floor(fu); let bv = floor(fv); let tu_ = fu - bu; let tv_ = fv - bv;
  var ms = array<u32, 4>(0u, 0u, 0u, 0u); var ws = array<f32, 4>(0.0, 0.0, 0.0, 0.0); var sum = 0.0;
  for (var k = 0u; k < 4u; k++) {
    let i = f32(k & 1u); let j = f32(k >> 1u);
    var c = q; c[au] = (bu + i + 0.5) * size; c[av] = (bv + j + 0.5) * size;
    let sl = lc_look_k(lc_key_s(c, n, M_PART, size)); if (sl > LC_MASK) { continue; }
    let mi = atomicLoad(&m_idx[sl]); if (mi == 0u || mi > M_MAPS) { continue; }
    let w = select(1.0 - tu_, tu_, (k & 1u) == 1u) * select(1.0 - tv_, tv_, (k >> 1u) == 1u) * min(m_out[mi - 1u][84].w / 16.0 / M_TRUST, 1.0);
    ms[k] = mi; ws[k] = w; sum += w;
  }
  if (sum <= 0.0) { return vec4<f32>(0.0); }
  var acc = vec3<f32>(0.0); let r0 = rnd(); let r1 = rnd();
  for (var s = 0u; s < 4u; s++) {
    let gs = ggx_dir(v, n, rough * rough, rough, f0, (f32(s & 1u) + r0) * 0.5, (f32(s >> 1u) + r1) * 0.5);
    if (gs.skip) { continue; }
    let uv = m_uv(gs.d, fr); var l = vec3<f32>(0.0); var lw = 0.0;
    for (var k = 0u; k < 4u; k++) { if (ws[k] <= 0.0) { continue; } let t = m_tap(ms[k] - 1u, uv, M_LV); if (t.w > 0.0) { l += t.xyz * ws[k]; lw += ws[k]; } }
    if (lw > 0.0) { acc += l / lw * gs.w; }
  }
  // (the spot against its cell: darker under a cabinet, nearer a wall. Only where both have filled, and only on metal, which shows nothing else: on a glazed tile wall's faint reflection the 10 cm cells' own unevenness drew tile-sized blotches beside a doorway)
  let fd = lcv_at(q, n, LCV_FINE, M_DC); let cd = lcv_at(q, n, M_SIZE, M_DC);
  var kr = 1.0;
  if (metal && fd.w > 0.5 && cd.w > 0.5) { kr = clamp(dot(fd.xyz / fd.w, vec3<f32>(0.333)) / max(dot(cd.xyz / cd.w, vec3<f32>(0.333)), 1e-4), 0.25, 4.0); }
  return vec4<f32>(acc * 0.25 * kr, min(sum, 1.0));
}
// ---- the light leaving a surface a ray landed on, the same for every ray that lands: what its cell holds, where it may be read (sl: its cell, or none where a cell's average would show); otherwise one sample of the sun, lamps and windows, and for the rest a path from there or e_near, a stand-in the caller has at hand
// (cheap: the sun alone by its ray, and e_near for all the rest -- a sharp reflection's many hits, where the full choice of lights cost a second a video in a tiled bathroom)
fn shade_hit(q: vec3<f32>, ng: vec3<f32>, mi: u32, sl: u32, path: bool, cheap: bool, e_near: vec3<f32>) -> vec3<f32> {
  let m = mats[mi];
  if (sl <= LC_MASK && lc_out[sl].w >= LC_MIN) { return m.emissive.rgb + m.albedo.rgb * lc_out[sl].xyz; }
  if (cheap) { return m.emissive.rgb + m.albedo.rgb * (p.sun_col.rgb * max(dot(ng, p.sun_dir.xyz), 0.0) * sun_vis(q, ng) / PI + e_near); }
  var eh = direct_one(q, ng, true, false);
  // (a cap on a stray bright sample, with room left for the sun's own light)
  let cap_ = 8.0 + 2.0 * max(max(p.sun_col.r, p.sun_col.g), p.sun_col.b) * max(dot(ng, p.sun_dir.xyz), 0.0) / PI;
  // (what a path finds here fills the cell, or a surface only reflections reach would never have one)
  if (path) { eh += gather(q, ng); if (sl <= LC_MASK) { lc_add(sl, eh); } }
  return m.emissive.rgb + m.albedo.rgb * (select(e_near, vec3<f32>(0.0), path) + eh * min(1.0, cap_ / max(max(max(eh.x, eh.y), eh.z), 1e-5)));
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
  let mi = atomicLoad(&m_idx[i]); if (mi > 0u && mi <= M_MAPS) { m_resolve(mi - 1u); }
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
// each lamp's share that a pixel traces apart, as trace's loop gives it (for passes that don't run that loop)
fn lamp_shares(o: vec3<f32>, n: vec3<f32>) {
  var ltotal = 0.0; var li = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    ltotal += dot(lights[li].col.xyz, vec3<f32>(0.333)) * max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    li++; }
  li = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    let c_ = max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    lamp_share[li] = clamp((dot(lights[li].col.xyz, vec3<f32>(0.333)) * c_ / max(ltotal, 1e-6) - 0.04) / 0.08, 0.0, 1.0);
    li++; }
}
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
    if ((u32(p.cam.w) & 64u) != 0u) { return seen; } // (a video: one a frame, its frames average them. A mirror's picture keeps no history, and takes a still's rays)
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
// how much of the sun's disc a point sees: spread evenly over the disc (a Fibonacci spiral, turned by a random angle per pixel) rather than at random. Eight first, round the rim (sun_rim_k); on a soft edge, where they disagree, 32. Two random rays agreed by chance across a wide penumbra, and its pixels came out wholly lit or wholly dark: a band of salt and pepper along a sunlit patch's edge
// the first look: 8 rays round the disc's rim. A penumbra's lit (or shaded) part starts as a crescent at the rim: 4 rays inside the disc all missed a thin one, and a soft frame shadow 0-20 % lit came out as specks of 0 among 0.15
fn sun_rim_k(k: u32, rot: f32) -> vec3<f32> {
  let r = p.scale.w * 0.96; let a = f32(k) * (PI / 4.0) + rot;
  return normalize(basis(p.sun_dir.xyz) * vec3<f32>(r * cos(a), r * sin(a), 1.0));
}
fn sun_dir_k(k: u32, nk: u32, rot: f32) -> vec3<f32> {
  let r = p.scale.w * sqrt((f32(k) + 0.5) / f32(nk)); let a = f32(k) * 2.39996 + rot;
  return normalize(basis(p.sun_dir.xyz) * vec3<f32>(r * cos(a), r * sin(a), 1.0));
}
fn sun_soft(o: vec3<f32>, n: vec3<f32>) -> f32 {
  if (dot(n, p.sun_dir.xyz) <= 0.0) { return 0.0; }
  let rot = rnd() * 2.0 * PI; var hit = 0u;
  for (var k = 0u; k < 8u; k++) { if (!occluded(o, sun_rim_k(k, rot), 1000.0)) { hit++; } }
  if (hit == 0u || hit == 8u) { return f32(hit) / 8.0; }
  hit = 0u;
  for (var k = 0u; k < 32u; k++) { if (!occluded(o, sun_dir_k(k, 32u, rot), 1000.0)) { hit++; } }
  return f32(hit) / 32.0;
}
// (and over the whole traced pixel, its finer surfaces each a point: from its middle alone, the fine shadow of a window's frame was caught at one spot of its pattern per pixel, and came out as specks where Blender, taking the whole pixel, has a smooth half-shadow)
fn sun_area(px: vec2<i32>, ss: i32, dims: vec2<f32>) -> f32 {
  let rot = rnd() * 2.0 * PI; var pts: array<vec3<f32>, 4>; var nrs: array<vec3<f32>, 4>; var np = 0u;
  for (var k = 0; k < 4; k++) {
    let gq = px * ss + vec2<i32>(k % ss, (k / ss) % ss) + vec2<i32>(select(0, ss / 2, ss == 1));
    let dq = textureLoad(g_depth, gq, 0); if (dq >= 1.0) { continue; }
    var nq = textureLoad(g_nrm, gq, 0).xyz; let pq = world_of(gq, dq, dims, p.inv_vp);
    if (dot(nq, nq) < 0.25) { nq = -normalize(pq - p.cam.xyz); }
    nq = normalize(nq); if (dot(nq, p.sun_dir.xyz) <= 0.0) { continue; }
    pts[np] = start_at(pq, nq); nrs[np] = nq; np++;
  }
  if (np == 0u) { return 0.0; }
  var hit = 0u;
  for (var k = 0u; k < 8u; k++) { if (!occluded(pts[k % np], sun_rim_k(k, rot), 1000.0)) { hit++; } }
  if (hit == 0u || hit == 8u) { return f32(hit) / 8.0; }
  hit = 0u;
  for (var k = 0u; k < 32u; k++) { if (!occluded(pts[k % np], sun_dir_k(k, 32u, rot), 1000.0)) { hit++; } }
  return f32(hit) / 32.0;
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
// the sun, the lamps and the windows as one list: one of them, chosen by what it would give here unshadowed, and one shadow ray for it. Three rays a hit before, one for each
// (each kind that gives anything keeps a tenth of the choices at least: what a lamp would give unshadowed says little about a window's sky behind it)
fn direct_one(o: vec3<f32>, n: vec3<f32>, sun_on: bool, rest: bool) -> vec3<f32> {
  let ndl = max(dot(n, p.sun_dir.xyz), 0.0);
  let es = select(0.0, dot(p.sun_col.rgb, vec3<f32>(0.333)) * ndl / PI, sun_on);
  var el = 0.0; var i = 0u;
  loop { if (i >= p.misc.z) { break; }
    let v = lights[i].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
    el += select(1.0, 1.0 - lamp_share[min(i, 63u)], rest) * dot(lights[i].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
    i++; }
  el *= p.scale.x;
  var ew = 0.0; i = 0u;
  loop { if (i >= p.misc2.z) { break; }
    let w = wins[i]; let v = w.c.xyz - o; let r2 = max(dot(v, v), 0.05); let l = v * inverseSqrt(r2);
    ew += dot(sky_at(l), vec3<f32>(0.333)) * w.c.w * max(dot(n, l), 0.03) * max(abs(dot(l, w.n.xyz)), 0.1) / (r2 * PI);
    i++; }
  let tot = es + el + ew; if (tot <= 0.0) { return vec3<f32>(0.0); }
  var ps = select(0.0, max(es / tot, 0.1), es > 0.0); var pl = select(0.0, max(el / tot, 0.1), el > 0.0); var pw = select(0.0, max(ew / tot, 0.1), ew > 0.0);
  let pt = ps + pl + pw; ps /= pt; pl /= pt; pw /= pt;
  let u = rnd();
  if (u < ps) { return p.sun_col.rgb * ndl * sun_vis(o, n) / (PI * ps); }
  if (u < ps + pl) { return lamp_rest(o, n, rest) / pl; }
  return portal(o, n) / max(pw, 1e-6);
}
// light gathered from the hemisphere over n, by paths (lit units: E / pi)
fn gather(o0: vec3<f32>, n0: vec3<f32>) -> vec3<f32> {
  g1_esc = false;
  var o = o0; var n = n0; var tp = vec3<f32>(1.0); var e = vec3<f32>(0.0);
  let bounces = p.misc.x;
  var lc_sl = array<u32, 2>(LC_MASK + 1u, LC_MASK + 1u); var lc_e = array<vec3<f32>, 2>(vec3<f32>(0.0), vec3<f32>(0.0)); var lc_t = lc_e;
  var lc_read = false; var lc_last = vec3<f32>(0.0); var lc_r = 0.0;
  // (the first of these is the first bounce: Blender's limit of 4 is 4 of them. One more ran every room's light a tenth high)
  for (var b = 0u; b < bounces; b++) {
    var d = cosine(n);
    if (b == 0u && g_use) { d = g_dir; tp *= g_wt; }
    let h = closest(o, d, 1000.0);
    if (b == 0u) { g1_pos = o + d * 1000.0; g1_nrm = vec3<f32>(0.0); }
    if (h.i < 0) { if (!through_window(o, d)) { e += tp * sky_at(d); } if (b == 0u) { g1_esc = true; } break; }
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
    let direct_here = direct_one(q, ng, true, false);
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

// ---- how much the light filter will have to average with here: the traced pixels within two that face the same way and lie on the same surface, weighted as its first round weighs them. About 1 on a flat surface; little on a moulding's curve, a rail, a frame's profile, where a pixel's few paths stay as streaks the filter cannot take out. Those take more paths
const THIN_T = 0.75; const THIN_MARK = 0.002; const THIN_K = 4u;
fn support(px: vec2<i32>, n: vec3<f32>, pos: vec3<f32>, w: i32, h: i32, ss: i32, dims: vec2<f32>) -> f32 {
  var kw = array<f32, 3>(0.375, 0.25, 0.0625); var ew = 0.0;
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    let q = px + vec2<i32>(i, j); if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2); let dq = textureLoad(g_depth, gq, 0); if (dq >= 1.0) { continue; }
    let nq = normalize(textureLoad(g_nrm, gq, 0).xyz + vec3<f32>(1e-5));
    ew += kw[abs(i)] * kw[abs(j)] * pow(max(dot(n, nq), 0.0), 32.0) * exp(-abs(dot(n, world_of(gq, dq, dims, p.inv_vp) - pos)) / 0.03);
  } }
  return ew;
}
@compute @workgroup_size(8, 8) fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let g = px * ss + vec2<i32>(ss / 2);
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(0.0)); textureStore(out_lamp, px, vec4<f32>(0.0)); if ((u32(p.cam.w) & 16u) == 0u) { vis_sh[u32(px.y * w + px.x)] = vec4<u32>(0u, 0xffffffffu, 0xffffffffu, 0xffffffffu); textureStore(out_sun, px, vec4<f32>(0.0)); } return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 73856093u) ^ (u32(px.y) * 19349663u) ^ (p.misc.y * 83492791u);
  let o = start_at(pos, n);
  let m_want = textureLoad(g_alb, g, 0).a >= 0.15; // (rough enough for a reflection to read the direction maps: this surface's cell takes one)
  // (the cache's first fill: one path from one pixel in each 4x4 block, so the full trace after it can already read the cache)
  if ((u32(p.cam.w) & 16u) != 0u) {
    var h_ = (u32(px.x >> 2u) * 73856093u) ^ (u32(px.y >> 2u) * 19349663u); h_ ^= h_ >> 13u; h_ *= 83492791u; h_ ^= h_ >> 16u;
    if (u32(px.x & 3) == (h_ & 3u) && u32(px.y & 3) == ((h_ >> 2u) & 3u)) { seed ^= 0x9e3779b9u; _ = gather(o, n); }
    return;
  }
  // (a video: settled, or an edge the cell can answer, takes no rays for its soft light. Every pixel still traces its nearest lamps as a still does: a hard shadow kept in history wears away, resampled every frame, and these rays have next to no noise)
  let lcv_v = (u32(p.cam.w) & 32u) != 0u && lc_on();
  var vid_read = false; var vcr = vec4<f32>(0.0); var lamp_keep = false;
  if (lcv_v) {
    let settled = (u32(p.cam.w) & 64u) != 0u && hist_settled(pos, n, w, h);
    lamp_keep = settled && (((u32(px.x) >> 3u) + (u32(px.y) >> 3u) * 3u + p.misc.y) & 3u) != 0u; // (settled: each 8x8 block of pixels every fourth frame is often enough. Whole blocks, as the GPU runs them: a block with one pixel tracing costs as if all did)
    vcr = lcv_read(pos, n);
    vid_read = settled || (vcr.w >= 0.7 && ((u32(p.cam.w) & 64u) == 0u || hist_around(pos, w, h)));
  }
  // the lamps that matter here, apart from the filtered light, which would wash out their shadows. Each comes in gradually with its share of the light: a fixed number of lamps per pixel let a lamp drop out all at once, and cut its shadow off in a straight line across a wall
  // (close up, with only the lamp's own size as a limit: a ceiling beside a lamp is really that bright, and a floor on the distance left it dull)
  var ltotal = 0.0; var li = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    ltotal += dot(lights[li].col.xyz, vec3<f32>(0.333)) * max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    li++; }
  var dl_ = vec3<f32>(0.0); li = 0u;
  var ent = array<u32, 6>(0xffffu, 0xffffu, 0xffffu, 0xffffu, 0xffffu, 0xffffu); var ne = 0u;
  loop { if (li >= min(p.misc.z, 64u)) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), LAMP_R * LAMP_R);
    let c_ = max(dot(n, v * inverseSqrt(r2)), 0.0) / r2;
    let f_ = clamp((dot(lights[li].col.xyz, vec3<f32>(0.333)) * c_ / max(ltotal, 1e-6) - 0.04) / 0.08, 0.0, 1.0);
    lamp_share[li] = f_;
    if (f_ > 0.0 && !lamp_keep) {
      let lv = lamp_vis(o, lights[li].pos.xyz);
      dl_ += f_ * lights[li].col.xyz * c_ * p.scale.x * lv;
      if (ne < 6u) { ent[ne] = (li << 8u) | u32(round(lv * 255.0)); ne++; } // (for its highlight: the same shadow)
    }
    li++; }
  // the sun, with the lamps at this traced pixel: two rays, and more only where they differ, on a shadow's soft edge. Compose spreads it over the finer pixels by surface, as it does the lamps
  var sv = 0.0;
  if (dot(n, p.sun_dir.xyz) > 0.0) {
    sv = sun_area(px, ss, dims);
  }
  textureStore(out_sun, px, vec4<f32>(sv, 0.0, 0.0, 1.0));
  vis_sh[u32(px.y * w + px.x)] = vec4<u32>(bitcast<u32>(sv), ent[0] | (ent[1] << 16u), ent[2] | (ent[3] << 16u), ent[4] | (ent[5] << 16u));
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
        thru += shade_hit(q, ng, t_.mat, select(LC_MASK + 1u, lc_look(q, ng, t_.mat), lc_on() && hh.t >= LC_SIZE), false, false, vec3<f32>(0.0));
      }
    }
    thru *= 0.5;
    thru_e = sheer * thru;
  }
  textureStore(out_lamp, px, select(vec4<f32>(dl_, gtao(px, g, pos, n, d, dims, w, h, ss)), vec4<f32>(-1.0, -1.0, -1.0, 1.0), lamp_keep)); // (below 0: history has it, with its occlusion)
  var e = vec3<f32>(0.0);
  // (a video: more paths where a surface has little history, just come into view or on a thin edge. What has been seen for frames needs few)
  var spp = p.dims.w;
  let thin = support(px, n, pos, w, h, ss, dims) < THIN_T;
  // (a video: the cache's answer, without a ray, where it has one. a 0 tells what follows that this pixel holds all its light. Elsewhere, paths, which fill the cache for next time)
  if (lcv_v) {
    if (vid_read) {
      if ((u32(p.cam.w) & 2u) != 0u) { resv[u32(px.y * w + px.x)] = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0); }
      textureStore(out_irr, px, vec4<f32>(vcr.xyz * (1.0 - max(sheer, 0.0)) + thru_e, -1.0)); return;
    }
    // (new to the picture: a final gather. A few rays from here, each ending in a cell where it lands, so no one cell can show as a patch)
    spp = 3u;
    // (a quarter of the gather, in its own dense pass: here only the light that comes straight from lamps and windows. A pass that skipped pixels within its blocks would cost as much as tracing them all)
    if ((u32(p.cam.w) & 1024u) != 0u) {
      // (one sample of them: the frames before it settles average them, as they do its gather)
      let ed = direct_one(o, n, false, true);
      resv[u32(px.y * w + px.x)] = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0);
      // (what is thin is marked, +2: ReSTIR gives it only its own 2x2's paths, which lie on the same strip. Its wider neighbours' would draw a streak along a moulding)
      textureStore(out_irr, px, vec4<f32>(ed * (1.0 - max(sheer, 0.0)) + thru_e, 1.0 - max(sheer, 0.0) + select(0.0, 2.0, thin))); return;
    }
  }
  if (thin && !lcv_v) { spp *= THIN_K; } // (more paths where its light becomes the history: a still, a video's first frame. Later frames give it its own few, unshared)
  var dep_ = vec3<f32>(0.0);
  let restir_ = (u32(p.cam.w) & 2u) != 0u;
  var rs = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), f32(spp), vec3<f32>(0.0), 0.0);
  for (var s = 0u; s < spp; s++) {
    if (restir_ && !thin) {
      let lp_ = direct_one(o, n, false, true); e += lp_; dep_ += lp_;
      if (lcv_v) { _ = guide_pick(lc_look_k(lc_key_s(pos, n, LCV_PART, LCV_COARSE)), n); }
      let w_used = g_wt;
      let lg = gather(o, n); let lc = lg * min(1.0, 40.0 / max(max(max(lg.x, lg.y), lg.z), 1e-5));
      if (lcv_v) { guide_add(lc_find_k(lc_key_s(pos, n, LCV_PART, LCV_COARSE)), n, normalize(g1_pos - o), lc / w_used); g_use = false; }
      dep_ += lc;
      if (m_on()) { m_sample(pos, n, normalize(g1_pos - o), g1_l(lg, w_used, normalize(g1_pos - o)), m_want, true); }
      let lm = dot(lc, vec3<f32>(0.333)); let wi = lm * PI;
      rs.wsum += wi;
      if (rnd() * rs.wsum < wi) { rs.pos = g1_pos; rs.nrm = g1_nrm; rs.rad = lc; rs.lp = lm; }
      continue;
    }
    let es = direct_one(o, n, false, true) + gather(o, n);
    // (only what cannot be real: a path that finds a sunlit patch is rare, valid, and most of a room's warm fill)
    let ec_ = es * min(1.0, 40.0 / max(max(max(es.x, es.y), es.z), 1e-5)); e += ec_; dep_ += ec_;
  }
  if (lcv_v) { lcv_add(pos, n, dep_ / f32(spp)); }
  // (what is thin keeps its own paths: ReSTIR would hand it its neighbours' along the moulding, one shared error drawn as a streak. An empty reservoir, and its own mark for ReSTIR to pass it by)
  if (restir_ && thin) { resv[u32(px.y * w + px.x)] = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0); textureStore(out_irr, px, vec4<f32>(e / f32(spp) * (1.0 - max(sheer, 0.0)) + thru_e, THIN_MARK)); return; }
  if (restir_) { resv[u32(px.y * w + px.x)] = rs; textureStore(out_irr, px, vec4<f32>(e / f32(spp) * (1.0 - max(sheer, 0.0)) + thru_e, 1.0 - max(sheer, 0.0))); return; }
  e = e / f32(spp) * (1.0 - max(sheer, 0.0)) + thru_e;
  textureStore(out_irr, px, vec4<f32>(e, 0.0));
}

// ---- a video's gather for what comes into view, at a quarter of the pixels: in each 2x2 the first new pixel, from a corner that turns each frame. Its paths go in its reservoir, for ReSTIR to share with the rest, and in the cache
@compute @workgroup_size(8, 8) fn gather_q(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let qp = vec2<i32>(id.xy) * 2;
  if (qp.x >= w || qp.y >= h) { return; }
  var px = vec2<i32>(-1, -1);
  for (var k = 0u; k < 4u; k++) {
    let j = (k + p.misc.y) & 3u; let c = qp + vec2<i32>(i32(j & 1u), i32(j >> 1u));
    if (c.x >= w || c.y >= h) { continue; }
    if (textureLoad(irr_done, c, 0).a >= 0.0 && textureLoad(g_depth, c * ss + vec2<i32>(ss / 2), 0) < 1.0) { px = c; break; }
  }
  if (px.x < 0) { return; }
  let g = px * ss + vec2<i32>(ss / 2);
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 73856093u) ^ (u32(px.y) * 19349663u) ^ (p.misc.y * 83492791u) ^ 0x5bd1e995u;
  let o = start_at(pos, n);
  let spp = 3u; let m_want = textureLoad(g_alb, g, 0).a >= 0.15;
  var dep_ = vec3<f32>(0.0);
  var rs = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), f32(spp), vec3<f32>(0.0), 0.0);
  for (var s = 0u; s < spp; s++) {
    _ = guide_pick(lc_look_k(lc_key_s(pos, n, LCV_PART, LCV_COARSE)), n);
    let w_used = g_wt;
    let lg = gather(o, n); let lc = lg * min(1.0, 40.0 / max(max(max(lg.x, lg.y), lg.z), 1e-5));
    guide_add(lc_find_k(lc_key_s(pos, n, LCV_PART, LCV_COARSE)), n, normalize(g1_pos - o), lc / w_used); g_use = false;
    dep_ += lc; if (m_on()) { m_sample(pos, n, normalize(g1_pos - o), g1_l(lg, w_used, normalize(g1_pos - o)), m_want, true); }
    let lm = dot(lc, vec3<f32>(0.333)); let wi = lm * PI;
    rs.wsum += wi;
    if (rnd() * rs.wsum < wi) { rs.pos = g1_pos; rs.nrm = g1_nrm; rs.rad = lc; rs.lp = lm; }
  }
  resv[u32(px.y * w + px.x)] = rs;
  lcv_add(pos, n, dep_ / f32(spp) + textureLoad(irr_done, px, 0).rgb);
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
  var vis = 0.0;
  let f = (vec2<f32>(px) + 0.5) / f32(ss) - 0.5 - sub_off(); // (a traced pixel's sample lies past its block's middle)
  let b = vec2<i32>(floor(f));
  var tw = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    tw += light_weight(b + vec2<i32>(i, j), f, n, pos, dims, ss, lw, lh);
  } }
  var e = vec3<f32>(-1.0, 0.0, 0.0);
  if (tw < 0.02) {
    // (the sun here too, since no traced pixel has it)
    vis = sun_soft(o, n);
    e = vec3<f32>(0.0);
    let spp = p.dims.w;
    for (var s = 0u; s < spp; s++) {
      let es = direct_one(o, n, false, false) + gather(o, n);
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
  seed = (u32(px.x) * 73856093u) ^ (u32(px.y) * 2654435761u) ^ 0x51ed270bu ^ select(0u, p.misc.y * 19349663u, (u32(p.cam.w) & 64u) != 0u);
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
  // (what this pixel kept last frame offers too, at most so many frames' worth: it would otherwise outweigh all that is new. The scene holds still, so the spot it found is still there)
  if (rs_time()) {
    let ip = rs_prev(pos, w, h);
    if (ip >= 0) {
      let rq = resv_a[off + u32(ip)];
      if (rq.m > 0.0 && rq.wsum > 0.0 && rq.lp > 0.0) {
        let lr = normalize(rq.pos - pos); let pr = dot(rq.rad * brdf_nl(n, vv, lr, rough, f0), vec3<f32>(0.333));
        if (pr > 0.0) { let sc = min(1.0, 20.0 * max(r0.m, 1.0) / rq.m); let wn = pr / rq.lp * rq.wsum * sc; wt += wn; mt += rq.m * sc; if (rnd() * wt < wn) { sel = rq; } }
      }
    }
    var keep_ = sel; keep_.wsum = wt; keep_.m = mt;
    if (sel.lp > 0.0) { keep_.lp = dot(sel.rad * brdf_nl(n, vv, normalize(sel.pos - pos), rough, f0), vec3<f32>(0.333)); }
    resv_b[off + u32(px.y * w + px.x)] = keep_;
  }
  var ind = vec3<f32>(0.0);
  if (sel.lp > 0.0 && wt > 0.0) {
    let v = sel.pos - pos; let l2 = dot(v, v); let l = v * inverseSqrt(max(l2, 1e-8));
    let fn_ = sel.rad * brdf_nl(n, vv, l, rough, f0); let ps = dot(fn_, vec3<f32>(0.333));
    if (ps > 0.0 && !occluded(o, l, sqrt(l2) - 0.02)) { ind = fn_ / ps * (wt / max(mt, 1.0)) / dfg_of(rough, max(dot(n, vv), 1e-3), f0); }
  }
  // (a video: what was traced here fills the reflection cache)
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
  if (dir_.a > 0.0 && dir_.a < 0.01) { textureStore(out_irr, px, dir_); return; } // (a thin surface's own paths)
  let g = px * ss + vec2<i32>(ss / 2); let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(out_irr, px, vec4<f32>(dir_.rgb, 0.0)); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  n = normalize(n);
  seed = (u32(px.x) * 2654435761u) ^ (u32(px.y) * 19349663u) ^ 0x9e3779b9u ^ select(0u, p.misc.y * 83492791u, (u32(p.cam.w) & 64u) != 0u);
  let o = start_at(pos, n);
  let r0 = resv[u32(px.y * w + px.x)];
  var sel = r0; var wt = r0.wsum; var mt = r0.m;
  // (a new pixel that did not gather: its own 2x2's paths first, then the neighbours' as for any)
  let qthin = dir_.a >= 2.0; let a_ = select(dir_.a, dir_.a - 2.0, qthin);
  let qmode = (u32(p.cam.w) & 1024u) != 0u && r0.m == 0.0 && dir_.a >= 0.0;
  for (var k = 0u; k < RS_K + 3u; k++) {
    var q: vec2<i32>;
    if (k < 3u) {
      if (!qmode) { continue; }
      let j = (u32(px.x & 1) + u32(px.y & 1) * 2u + 1u + k) & 3u; q = (px / 2) * 2 + vec2<i32>(i32(j & 1u), i32(j >> 1u));
    } else {
      if (qthin) { continue; }
      let a = rnd() * 2.0 * PI; let rr = sqrt(rnd()) * RS_RADIUS;
      q = px + vec2<i32>(round(vec2<f32>(cos(a), sin(a)) * rr));
    }
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
  if (rs_time()) {
    let ip = rs_prev(pos, w, h);
    if (ip >= 0) {
      let rq = resv_a[u32(ip)];
      if (rq.m > 0.0 && rq.wsum > 0.0 && dot(n, rq.pos - pos) > 0.0) { let sc = min(1.0, 20.0 * max(r0.m, 1.0) / rq.m); let wn = rq.wsum * sc; wt += wn; mt += rq.m * sc; if (rnd() * wt < wn) { sel = rq; } }
    }
    var keep_ = sel; keep_.wsum = wt; keep_.m = mt; resv_b[u32(px.y * w + px.x)] = keep_;
  }
  var ind = vec3<f32>(0.0);
  if (sel.lp > 0.0) {
    let v = sel.pos - pos; let l2 = dot(v, v); let l = v * inverseSqrt(max(l2, 1e-8)); let c = dot(n, l);
    if (c > 0.0 && !occluded(o, l, sqrt(l2) - 0.02)) { ind = sel.rad / sel.lp * (wt / max(mt, 1.0)) / PI; }
  }
  // (nothing to share, across an edge: the cache's answer, as an edge's pixel takes it)
  if (qmode && wt <= 0.0) { let cr_ = lcv_read(pos, n); if (cr_.w >= 0.3) { textureStore(out_irr, px, vec4<f32>(cr_.xyz, -1.0)); return; } }
  textureStore(out_irr, px, vec4<f32>(dir_.rgb + ind * max(a_, 0.0), a_)); // (a keeps saying whether this pixel traced: 0 is the cache's clean answer, which the history and the filter treat differently)
}

// GTAO, as games do it: how far the surfaces nearby rise above this point, searched in the depth picture along a few directions. For each side of a direction a share sin^2 of the cosine-weighted hemisphere is closed off. Cheap, and the darkening of corners and contacts stays sharp, where the light's filter smooths it away
const AO_R = 0.5; // m
fn gtao(px: vec2<i32>, g: vec2<i32>, pos: vec3<f32>, n: vec3<f32>, d: f32, dims: vec2<f32>, w: i32, h: i32, ss: i32) -> f32 {
  let wp = max(length(world_of(g + vec2<i32>(ss, 0), d, dims, p.inv_vp) - pos), 1e-4);
  let rpx = clamp(AO_R / wp, 3.0, 40.0);
  // (a video turns it a little each frame, or its pattern never averages out)
  let rot = fract(52.9829189 * fract(dot(vec2<f32>(px), vec2<f32>(0.06711056, 0.00583715))) + select(0.0, f32(p.misc.y) * 0.61803399, (u32(p.cam.w) & 64u) != 0u)) * PI;
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
fn lamp_glint(pos: vec3<f32>, n: vec3<f32>, o: vec3<f32>, g: vec2<i32>, rough: f32, w: i32, h: i32, ss: i32, e: vec3<f32>, sh: vec4<u32>) -> vec3<f32> {
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
  var cs = array<vec3<f32>, 7>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
  var ls = array<vec3<f32>, 7>(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
  var ms = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); var ds = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0); var is_ = array<u32, 7>(0u, 0u, 0u, 0u, 0u, 0u, 0u); var i = 0u;
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
      var k = 6;
      if (m_ > ms[6]) {
        loop { if (k == 0 || m_ <= ms[k - 1]) { break; } ms[k] = ms[k - 1]; cs[k] = cs[k - 1]; ls[k] = ls[k - 1]; ds[k] = ds[k - 1]; is_[k] = is_[k - 1]; k--; }
        ms[k] = m_; cs[k] = c; ls[k] = l; ds[k] = dl; is_[k] = i;
      }
    }
    i++; }
  // (the brightest lamps, each faded out as it nears the first one left out: a hard cut at the Kth made a lamp's whole highlight start and stop along a ragged line wherever two lamps changed places. The fade is continuous, so nothing changes at a swap)
  var out = vec3<f32>(0.0);
  for (var k = 0; k < 6; k++) {
    if (ds[k] <= 0.0 || ms[k] <= 0.002) { continue; }
    let wk = clamp(2.0 * (1.0 - ms[6] / ms[k]), 0.0, 1.0);
    if (wk <= 0.0) { continue; }
    // (the shadow the trace found for this lamp's light, where it traced it: one shadow for both, and no second ray)
    let sv_ = vis_lamp(sh, is_[k]);
    if (sv_ >= 0.0) { out += cs[k] * wk * sv_; } else if (!occluded(o, ls[k], ds[k] - 0.02)) { out += cs[k] * wk; }
  }
  return min(out, vec3<f32>(50.0));
}
// ---- one direction from a rough surface's lobe, the same for a ray and for a read of the direction maps: GGX by its visible normals (Heitz 2018), weighted against the share compose multiplies by (F G2/G1 over the lobe's DFG). A narrow cone left out most of what a rough surface reflects: dark blades and metal doors
struct GS { d: vec3<f32>, w: vec3<f32>, skip: bool };
fn ggx_dir(v: vec3<f32>, n: vec3<f32>, al_: f32, rough: f32, f0_: vec3<f32>, u_: f32, u2: f32) -> GS {
  let r0 = reflect(v, n);
  if (al_ <= 0.001) { return GS(select(r0, r0, dot(r0, n) > 0.0), vec3<f32>(1.0), false); }
  let vl = -v; let bs_ = basis(n);
  let ve = vec3<f32>(dot(vl, bs_[0]), dot(vl, bs_[1]), dot(vl, bs_[2]));
  let vh = normalize(vec3<f32>(al_ * ve.x, al_ * ve.y, ve.z));
  let lsq = vh.x * vh.x + vh.y * vh.y;
  let t1_ = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-vh.y, vh.x, 0.0) * inverseSqrt(max(lsq, 1e-9)), lsq > 1e-9);
  let t2_ = cross(vh, t1_);
  let r_ = sqrt(u_); let ph = 2.0 * PI * u2;
  let t1s = r_ * cos(ph); let sv = 0.5 * (1.0 + vh.z);
  let t2s = (1.0 - sv) * sqrt(max(1.0 - t1s * t1s, 0.0)) + sv * r_ * sin(ph);
  let nh = t1s * t1_ + t2s * t2_ + sqrt(max(0.0, 1.0 - t1s * t1s - t2s * t2s)) * vh;
  let ne = normalize(vec3<f32>(al_ * nh.x, al_ * nh.y, max(0.0, nh.z)));
  let h_w = bs_ * ne;
  let rd = reflect(v, h_w);
  let nl = dot(rd, n);
  if (nl <= 0.0) { return GS(r0, vec3<f32>(0.0), true); }
  let vdh = max(dot(vl, h_w), 0.0); let nv_ = max(dot(n, vl), 1e-3);
  let fh = f0_ + (vec3<f32>(1.0) - f0_) * pow(1.0 - vdh, 5.0);
  let a2 = al_ * al_;
  let g1v = 2.0 * nv_ / (nv_ + sqrt(a2 + (1.0 - a2) * nv_ * nv_));
  let g1l = 2.0 * nl / (nl + sqrt(a2 + (1.0 - a2) * nl * nl));
  let g2g1 = g1l / max(g1v + g1l - g1v * g1l, 1e-3);
  let ab_ = dfg_ab(rough, nv_);
  return GS(rd, fh * g2g1 / max(f0_ * ab_.x + vec3<f32>(ab_.y), vec3<f32>(0.005)), false);
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
  var hit_d = 0.0; var hit_n = 1.0;
  var sp = vec3<f32>(0.0); var rs_s = R(vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0, vec3<f32>(0.0), 0.0);
  let coat = textureLoad(g_nrm, g, 0).a;
  if (p.misc2.y > 0u) {
    let v = normalize(pos - p.cam.xyz);
    let r0 = reflect(v, n);
    // (a flat mirror, alb's a 0: what it shows was traced already, as a picture from the camera mirrored in it, with textures and its own light)
    let is_mirror = rough < 0.001;
    // (metal shows nothing but its reflections: full mode gives it one more ray)
    // (and what is rough enough reads the reflection cache, filled by the first frame: no rays, so no grain. hit_d < 0 tells the history. Only what the cache hasn't seen yet traces, and fills it)
    var turn = false;
    // (not on what is curved or thin, nor on smooth metal: a cell's blur is all you would see, and a faucet or a sink's rim went flat)
    let m_lo = 0.35; // (faded per pixel: a sharp limit drew the roughness map's contours across a sink)
    let sl_ = textureLoad(g_mat, g, 0).y; let f0_rs = mix(vec3<f32>(0.08 * select(sl_, 0.5, sl_ <= 0.0 || sl_ > 4.0)), a.rgb, textureLoad(g_mat, g, 0).x); // (F0 by the specular level, as compose has it)
    let is_m = textureLoad(g_mat, g, 0).x >= 0.5;
    var flat_ = true;
    if (is_m) { for (var k = 0; k < 4; k++) {
      var o4 = array<vec2<i32>, 4>(vec2<i32>(2, 0), vec2<i32>(-2, 0), vec2<i32>(0, 2), vec2<i32>(0, -2));
      let gq = clamp(g + o4[k] * ss, vec2<i32>(0), vec2<i32>(w * ss - 1, h * ss - 1));
      let nq = textureLoad(g_nrm, gq, 0).xyz; if (dot(nq, nq) < 0.25 || dot(normalize(nq), n) < 0.985) { flat_ = false; }
    } }
    let m_ok = !is_m || (u32(p.cam.w) & 64u) == 0u || rnd() < smoothstep(m_lo, m_lo + 0.2, rough); // (no draw unless it will be used: a still's noise stays as it was)
    if ((u32(p.cam.w) & 64u) != 0u && !is_mirror && flat_ && m_ok && rough >= 0.15) {
      let rc = m_read(pos, n, v, rough, f0_rs, is_m);
      if (rc.w >= 0.7) { sp = rc.xyz; turn = true; hit_d = -1.0; }
    }
    let rays = select(select(select(p.misc2.y, 4u, p.misc2.y >= 3u && textureLoad(g_mat, g, 0).x >= 0.5), 0u, is_mirror), 0u, turn);
    let use_rs = (u32(p.cam.w) & 4u) != 0u && rough > 0.15 && !is_mirror;
    let m_feed = m_on() && rough >= 0.15 && !is_mirror; // (what the rays find here fills its cell's map: the cache fills where reflections look)
    rs_s.m = select(0.0, f32(rays), use_rs); hit_n = max(f32(rays), 1.0); // (rays kept to itself are no samples for the neighbours to count: a rough floor beside smoother patches came out darker there)
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
        sp += textureLoad(m_lit, clamp(px + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(w - 1, h - 1)) * vec2<i32>(textureDimensions(m_lit)) / vec2<i32>(w, h), 0).rgb * f32((2 - abs(i)) * (2 - abs(j))) / 16.0;
      } }
    }
    let al_ = min(rough * rough + spec_w, 1.0);
    for (var s = 0u; s < rays; s++) {
      // the rough lobe as it really is (GGX, visible normals, Heitz 2018), each ray weighted against the share the composition step will multiply by. A narrow cone left out most of what a rough surface reflects: dark blades and metal doors
      let u1_ = rnd(); let u2_ = rnd();
      let gs_ = ggx_dir(v, n, al_, rough, f0_rs, u1_, u2_);
      if (gs_.skip) { continue; }
      let rd = gs_.d; let wt_ = gs_.w;
      let hh = closest(o, rd, 1000.0);
      hit_d += select(min(hh.t, 30.0), 30.0, hh.i < 0);
      if (hh.i < 0) {
        var l = sky_at(rd);
        // (the sun only in a truly sharp reflection: on anything rough its highlight is already counted, and a ray that happened on it was a firefly)
        if (rough < 0.03 && dot(rd, p.sun_dir.xyz) > cos(p.scale.w)) { l += p.sun_col.rgb * (1.0 / (2.0 * PI * (1.0 - cos(p.scale.w)))); }
        if (m_feed) { m_sample(pos, n, rd, l, true, false); }
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
        // (metal, rough: a real path from where it landed, as all it shows is what it reflects. Rough: one sample of the lights, and half this pixel's own light for the bounce. Sharp, or one ray a pixel: the sun, and this pixel's own light for the rest)
        let full_ = rough > 0.15 && (p.misc2.y > 1u || on_metal);
        let lh_ = shade_hit(q, ng, t.mat, sl, on_metal && rough > 0.15, !full_, select(e, 0.5 * e, full_));
        if (m_feed) { m_sample(pos, n, rd, lh_, true, false); }
        if (use_rs) { rs_add(&rs_s, q, ng, lh_, wt_, n, -v, rd, rough, f0_rs); continue; }
        sp += lh_ * wt_;
      }
    }
    if (rays > 0u) { sp /= f32(rays); }
  }
  textureStore(out_lamp, px, vec4<f32>(lamp_smooth(px, pos, n, dims, w, h, ss), ao_fine(px, pos, n, dims, w, h, ss)));
  // (not through the reflections' filter: it smeared the shadow each louvre blade casts on the next. Except where roughness comes from a map: its jumps drew hard shapes across a door and blotches on a floor)
  var gl_ = lamp_glint(pos, n, o, g, rough, w, h, ss, e, vis_sh[u32(px.y * w + px.x)]);
  if (rough > 0.08 && textureLoad(g_mat, g, 0).w >= 2.9 && coat < 0.25 && p.misc2.y > 0u) {
    let vl_ = normalize(p.cam.xyz - pos); let nv_g = max(dot(n, vl_), 1e-3);
    let abg = dfg_ab(rough, nv_g);
    let slg_ = textureLoad(g_mat, g, 0).y; let f0g = mix(vec3<f32>(0.08 * select(slg_, 0.5, slg_ <= 0.0 || slg_ > 4.0)), a.rgb, textureLoad(g_mat, g, 0).x);
    sp += min(gl_ / max(f0g * abg.x + vec3<f32>(abg.y), vec3<f32>(0.005)), vec3<f32>(20.0)); gl_ = vec3<f32>(0.0); // (into the reflection, whether rays or the maps found it: the maps hold no lamps, and a floor lost its lamp's highlight once they covered it)
  }
  textureStore(out_glint, px, vec4<f32>(gl_, hit_d / hit_n)); // (a: how far the rays went, for a video's history)
  if ((u32(p.cam.w) & 4u) != 0u) { resv[u32(w * h) + u32(px.y * w + px.x)] = rs_s; }
  textureStore(out_spec, px, vec4<f32>(min(sp, vec3<f32>(20.0)), rough));
}

// a video's first frame, clean: its light goes into the cells later frames read
@compute @workgroup_size(8, 8) fn lc_seed(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let g = px * ss + vec2<i32>(ss / 2);
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { return; }
  let pos = world_of(g, d, vec2<f32>(f32(w * ss), f32(h * ss)), p.inv_vp);
  var n = textureLoad(g_nrm, g, 0).xyz;
  if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); }
  lcv_add(pos, normalize(n), textureLoad(irr_done, px, 0).rgb); // (the soft light alone, as a video's frames add it: the lamps traced apart come on top. With them in, a mirror's picture, which reads the cells and traces its lamps too, had them twice, ~15% too bright)
}



// ---- a video fills the cache's coarse cells ahead of the camera: samples of the surfaces round it, so a pan or a turn finds them lit. A cell already full is skipped at once, so this costs less as the cache fills
fn fill_at(dir: vec3<f32>) {
  let h = closest(p.cam.xyz, dir, 1000.0);
  if (h.i < 0) { return; }
  let t = tris[h.i];
  var ng = normalize(cross(t.e1.xyz, t.e2.xyz));
  if (dot(ng, dir) > 0.0) { ng = -ng; }
  let q = p.cam.xyz + dir * h.t + ng * 0.002;
  if (lcv_at(q, ng, LCV_COARSE, LCV_PART).w >= 0.999) { return; }
  let o = q + ng * 0.001;
  // (a cell stands for this hit and 3 more bounces, and gather goes 4 from here: the same total as Blender's 4, in a room of one colour)
  let r_ = dot(mats[t.mat].albedo.rgb, vec3<f32>(0.333)); let s_ = 1.0 + r_ + r_ * r_;
  let cg = lc_find_k(lc_key_s(q, ng, LCV_PART, LCV_COARSE));
  _ = guide_pick(cg, ng); let w_used = g_wt;
  let lg = gather(o, ng); g_use = false;
  guide_add(cg, ng, normalize(g1_pos - o), lg / w_used);
  // (two direct lights, for two caches: the path cache's cells have every lamp, for the bounces that read them; the camera's cells the soft light alone, without the lamps a pixel traces apart. One value for both put every lamp in the camera cells, which the frame then traced again; with the shares set for both, the bounces lost the lamps, and what a pan came round to was 25% dark)
  let dl1 = direct_one(o, ng, false, false);
  lamp_shares(o, ng); let dl_rest = direct_one(o, ng, false, true);
  if (m_on()) { m_sample(q, ng, normalize(g1_pos - o), g1_l(lg, w_used, normalize(g1_pos - o)), false, true); }
  let l = dl1 + lg * s_ / (s_ + r_ * r_ * r_);
  let lc = l * min(1.0, 40.0 / max(max(max(l.x, l.y), l.z), 1e-5));
  // (the camera's cells take the whole gather, as trace and gather_q give them: the rescale above is the path cache's, whose cells stand for this hit and 3 more. With it here, what a pan came round to started a bounce short, darker)
  let lv_ = dl_rest + lg; lcv_add_coarse(q, ng, lv_ * min(1.0, 40.0 / max(max(max(lv_.x, lv_.y), lv_.z), 1e-5)));
  let sl = lc_find(lc_jit(q, ng), ng, t.mat);
  if (sl <= LC_MASK) { lc_add(sl, lc + p.sun_col.rgb * max(dot(ng, p.sun_dir.xyz), 0.0) * sun_vis(o, ng) / PI); }
}
const FILL_REACH = 2.6; // (how far past the picture's edge, in the picture's own half-widths)
// every frame: just outside the picture, where a turn is heading
@compute @workgroup_size(8, 8) fn lc_fill_view(@builtin(global_invocation_id) id: vec3<u32>) {
  seed = (id.x * 73856093u) ^ (id.y * 19349663u) ^ (p.misc.y * 83492791u); seed = seed * 747796405u + 2891336453u;
  var ndc = vec2<f32>(0.0);
  for (var k = 0; k < 3; k++) { ndc = vec2<f32>(rnd() * 2.0 - 1.0, rnd() * 2.0 - 1.0) * FILL_REACH; if (max(abs(ndc.x), abs(ndc.y)) > 1.0) { break; } }
  let w4 = p.inv_vp * vec4<f32>(ndc, 0.9999, 1.0);
  fill_at(normalize(w4.xyz / w4.w - p.cam.xyz));
}
// the first frame: all round, as far as the room goes
@compute @workgroup_size(8, 8) fn lc_fill_room(@builtin(global_invocation_id) id: vec3<u32>) {
  seed = (id.x * 73856093u) ^ (id.y * 19349663u) ^ 0x68e31da4u; seed = seed * 747796405u + 2891336453u;
  let z = rnd() * 2.0 - 1.0; let a = rnd() * 2.0 * PI; let r = sqrt(max(1.0 - z * z, 0.0));
  fill_at(vec3<f32>(r * cos(a), r * sin(a), z));
}
