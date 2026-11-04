// bake: appended to rt.wgsl, whose traversal, sky, lamps and windows it uses. The light is in a world-space cache: cells keyed by position and the side a surface faces, light without the surface's colour;
// a frame looks it up per pixel and traces only what the cache cannot hold (the sun's own edges, sharp reflections).
struct BP {
  lo: vec4<f32>,      // the house's box, w: size of the finest cell
  hi: vec4<f32>,      // w: the look (AgX contrast)
  grid: vec4<u32>,    // nx, ny, table mask, table shift
  run: vec4<u32>,     // round, offset, count, samples
  fl: vec4<f32>,      // alpha of this round / of the history, exposure, frame-in-history flag, stride
  scales: vec4<f32>,  // lamp, sky, sun, -
  prev_vp: mat4x4<f32>,
  prev_eye: vec4<f32>,
  frame: vec4<u32>,   // frame, spec offset x, y, clamp history (1)
};
// e: what the cache gives a pixel, all but the lamps' direct light. e_hit: with it, for where a reflection lands. any, all: the first 32 lamps seen from somewhere on this cell / from all of it
struct Slot { key: u32, e: u32, vis: f32, id: u32, any: u32, all: u32, e_hit: u32, pad: u32 };
struct Pt { pos: vec3<f32>, nrm: u32 };
// sun: direct, a = how much of it gets here. sky, lamp: direct. i_*: what arrives by way of other surfaces
struct CellV { sun: vec4<f32>, sky: vec4<f32>, lamp: vec4<f32>, i_sun: vec4<f32>, i_sky: vec4<f32>, i_lamp: vec4<f32> };
const EMPTY = 0xffffffffu;
const K_PTS = 4u;
@group(1) @binding(0) var<uniform> b: BP;
@group(1) @binding(1) var<storage, read> table: array<Slot>;
@group(1) @binding(2) var<storage, read> pts: array<Pt>;
@group(1) @binding(3) var<storage, read> cur: array<CellV>;
@group(1) @binding(4) var<storage, read_write> nxt: array<CellV>;
@group(1) @binding(5) var<storage, read_write> flags: array<u32>;
@group(1) @binding(6) var<storage, read_write> out_tbl: array<Slot>;
@group(1) @binding(7) var<storage, read_write> lm_out: array<vec2<u32>>;
@group(1) @binding(8) var<storage, read> lm: array<vec2<u32>>;

fn dec_n(v: u32) -> vec3<f32> {
  let f = vec2<f32>(f32(v & 0xffffu), f32(v >> 16u)) / 65535.0 * 2.0 - 1.0;
  var n = vec3<f32>(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
  if (n.z < 0.0) { n = vec3<f32>((1.0 - abs(n.y)) * select(-1.0, 1.0, n.x >= 0.0), (1.0 - abs(n.x)) * select(-1.0, 1.0, n.y >= 0.0), n.z); }
  return normalize(n);
}
fn level_of(q: vec3<f32>) -> u32 {
  let d = max(max(b.lo.xyz - q, q - b.hi.xyz), vec3<f32>(0.0));
  let m = max(d.x, max(d.y, d.z));
  if (m <= 0.0) { return 0u; } if (m < 16.0) { return 1u; } if (m < 128.0) { return 2u; }
  return 3u;
}
fn cell_of(l: u32) -> f32 { return b.lo.w * f32(1u << (3u * l)); }
fn org_of(l: u32) -> vec3<f32> { if (l == 0u) { return b.lo.xyz; } return vec3<f32>(-cell_of(l) * 512.0, -cell_of(l) * 512.0, -cell_of(l) * 32.0); }
fn key_of(l: u32, c: vec3<i32>, side: u32) -> u32 {
  if (l == 0u) {
    if (c.x < 0 || c.y < 0 || c.z < 0 || u32(c.x) >= b.grid.x || u32(c.y) >= b.grid.y) { return EMPTY; }
    return (u32(c.x) + b.grid.x * (u32(c.y) + b.grid.y * u32(c.z))) * 6u + side;
  }
  return 0x80000000u | ((l - 1u) << 29u) | ((u32(c.x) & 1023u) | ((u32(c.y) & 1023u) << 10u) | ((u32(c.z) & 63u) << 20u) | (side << 26u));
}
fn find(key: u32, id: u32) -> i32 {
  if (key == EMPTY) { return -1; }
  var h = key ^ (id * 0x9E3779B1u); h ^= h >> 15u; var s = (h * 2654435761u) >> b.grid.w;
  for (var k = 0; k < 32; k++) {
    let t = table[s];
    if (t.key == key && t.id == id) { return i32(s); }
    if (t.key == EMPTY) { return -1; }
    s = (s + 1u) & b.grid.z;
  }
  return -1;
}
fn side_of(n: vec3<f32>) -> u32 {
  let a = abs(n);
  if (a.x >= a.y && a.x >= a.z) { return select(1u, 0u, n.x >= 0.0); }
  if (a.y >= a.z) { return select(3u, 2u, n.y >= 0.0); }
  return select(5u, 4u, n.z >= 0.0);
}
// the cell at a point on a surface facing n (a surface exactly on a cell's boundary may have gone to the next layer; one side's cells may not exist where another's do)
fn cell_at(q: vec3<f32>, n: vec3<f32>, id: u32) -> i32 {
  let l = level_of(q); let c = cell_of(l); let o = org_of(l); let sd = side_of(n);
  let ax = sd / 2u; let dir = select(1, -1, (sd & 1u) == 1u);
  var ic = vec3<i32>(floor((q - o) / c));
  var s = find(key_of(l, ic, sd), id); if (s >= 0) { return s; }
  var t = ic; t[ax] -= dir;
  s = find(key_of(l, t, sd), id); if (s >= 0) { return s; }
  t = ic; t[ax] += dir;
  return find(key_of(l, t, sd), id);
}

// ---- the bake
@compute @workgroup_size(64) fn bk_validate(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x + b.run.y; if (i >= b.run.z) { return; }
  seed = i * 73856093u + 19349663u;
  let pt = pts[i]; let n = dec_n(pt.nrm); let o = pt.pos + n * 0.002;
  // (inside a solid; or lying against something, as a floor under a rug: never seen, and dark)
  var back = 0u; var near = 0u;
  for (var s = 0u; s < 6u; s++) {
    let d = cosine(n);
    let h = closest(o, d, 1000.0);
    if (h.i >= 0) { let t = tris[h.i]; if (dot(cross(t.e1.xyz, t.e2.xyz), d) > 0.0) { back += 1u; } if (h.t < 0.03) { near += 1u; } }
  }
  flags[i] = select(1u, 0u, back >= 4u || near >= 5u);
}
@compute @workgroup_size(64) fn bk_round(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x + b.run.y; if (i >= b.run.z) { return; }
  let round = b.run.x;
  seed = (i * 2654435761u) ^ (round * 83492791u); seed = seed * 747796405u + 2891336453u;
  var sun = vec3<f32>(0.0); var vis = 0.0; var sky = vec3<f32>(0.0); var lmp = vec3<f32>(0.0); var isun = vec3<f32>(0.0); var isky = vec3<f32>(0.0); var ilmp = vec3<f32>(0.0);
  let ns = b.run.w; let c0 = cur[i];
  for (var k = 0u; k < ns; k++) {
    let pt = pts[i * K_PTS + (k + round) % K_PTS];
    let n = dec_n(pt.nrm); let o0 = pt.pos + n * 0.002;
    // (anywhere on the cell, so that it holds the mean over its area and a shadow's edge crossing cells comes out smooth, not in teeth of what its few points happened to see. Not past something standing on the surface)
    let bs = basis(n); let cz = cell_of(level_of(pt.pos));
    let dj = bs[0] * (rnd() - 0.5) * cz + bs[1] * (rnd() - 0.5) * cz; let lj = length(dj);
    var o = o0;
    if (lj > 1e-5 && !occluded(o0, dj / lj, lj)) { o = o0 + dj; }
    let v = sun_vis(o, n); vis += v;
    sun += p.sun_col.rgb * max(dot(n, p.sun_dir.xyz), 0.0) * v / PI;
    lmp += lamp(o, n); sky += portal(o, n);
    let d = cosine(n);
    let h = closest(o, d, 1000.0);
    if (h.i < 0) { if (!through_window(o, d)) { isky += sky_at(d); } continue; }
    let t = tris[h.i]; let m = mats[t.mat];
    var ng = normalize(cross(t.e1.xyz, t.e2.xyz)); if (dot(ng, d) > 0.0) { ng = -ng; }
    let q = o + d * h.t + ng * 0.002;
    ilmp += m.emissive.rgb;
    let s = cell_at(q, ng, t.mat % 2047u);
    // (a point no cell holds: nearly always beside this one)
    let cv = cur[select(i, table[max(s, 0)].e, s >= 0)];
    isky += m.albedo.rgb * (cv.sky.rgb + cv.i_sky.rgb); ilmp += m.albedo.rgb * (cv.lamp.rgb + cv.i_lamp.rgb);
    isun += m.albedo.rgb * (cv.sun.rgb + cv.i_sun.rgb);
  }
  let w = 1.0 / f32(ns); let al = b.fl.x;
  // (the last rounds count most: the first ones saw a cache still dark)
  nxt[i] = CellV(mix(c0.sun, vec4<f32>(sun * w, vis * w), al), mix(c0.sky, vec4<f32>(sky * w, 0.0), al), mix(c0.lamp, vec4<f32>(lmp * w, 0.0), al), mix(c0.i_sun, vec4<f32>(isun * w, 0.0), al), mix(c0.i_sky, vec4<f32>(isky * w, 0.0), al), mix(c0.i_lamp, vec4<f32>(ilmp * w, 0.0), al));
}
// which lamps a cell sees. A pixel traces only those some of its cell sees and some not: their shadows are sharp, and no cell can hold them
@compute @workgroup_size(64) fn bk_lamps(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x + b.run.y; if (i >= b.run.z) { return; }
  seed = (i * 2654435761u) ^ 0x5bd1e995u; seed = seed * 747796405u + 2891336453u;
  var any_ = 0u; var all_ = 0u; let nl = min(p.misc.z, 32u);
  for (var l = 0u; l < nl; l++) {
    var hit = 0u; var tried = 0u;
    for (var k = 0u; k < 12u; k++) {
      let pt = pts[i * K_PTS + k % K_PTS]; let n = dec_n(pt.nrm); let o0 = pt.pos + n * 0.002;
      let bs = basis(n); let cz = cell_of(level_of(pt.pos));
      let dj = bs[0] * (rnd() - 0.5) * cz + bs[1] * (rnd() - 0.5) * cz; let lj = length(dj);
      var o = o0; if (lj > 1e-5 && !occluded(o0, dj / lj, lj)) { o = o0 + dj; }
      let v = lights[l].pos.xyz - o; let r2 = max(dot(v, v), 0.0004); let d = v * inverseSqrt(r2);
      if (dot(n, d) <= 0.0) { continue; }
      tried += 1u;
      if (!occluded(o, d, sqrt(r2) - 0.02)) { hit += 1u; }
    }
    if (hit > 0u) { any_ |= 1u << l; if (hit == tried && tried == 12u) { all_ |= 1u << l; } }
  }
  lm_out[i] = vec2<u32>(any_, all_);
}
// light that came by way of other surfaces is smooth, and the rare path that finds a sunlit patch is not: it is averaged along the surface. A pass: 5x5 cells of this part and side, spaced by run.x
@compute @workgroup_size(64) fn bk_filter(@builtin(global_invocation_id) id: vec3<u32>) {
  let s0 = id.x + b.run.y; if (s0 >= b.run.z) { return; }
  let t = table[s0]; if (t.key == EMPTY) { return; }
  let c = cur[t.e];
  var l = 0u; var ic: vec3<i32>; var sd: u32;
  if ((t.key & 0x80000000u) == 0u) {
    sd = t.key % 6u; let q = t.key / 6u; ic = vec3<i32>(i32(q % b.grid.x), i32((q / b.grid.x) % b.grid.y), i32(q / (b.grid.x * b.grid.y)));
  } else {
    l = ((t.key >> 29u) & 3u) + 1u; sd = (t.key >> 26u) & 7u; ic = vec3<i32>(i32(t.key & 1023u), i32((t.key >> 10u) & 1023u), i32((t.key >> 20u) & 63u));
  }
  let ax = sd / 2u; let au = (ax + 1u) % 3u; let av = (ax + 2u) % 3u; let st = i32(b.run.x);
  var a = vec3<f32>(0.0); var e = vec3<f32>(0.0); var f = vec3<f32>(0.0); var dsky = vec3<f32>(0.0); var w = 0.0;
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    var q = ic; q[au] += i * st; q[av] += j * st;
    let s = find(key_of(l, q, sd), t.id);
    if (s < 0) { continue; }
    let k = array<f32, 3>(0.375, 0.25, 0.0625)[abs(i)] * array<f32, 3>(0.375, 0.25, 0.0625)[abs(j)];
    let v = cur[table[s].e];
    a += v.i_sun.rgb * k; e += v.i_sky.rgb * k; f += v.i_lamp.rgb * k; dsky += v.sky.rgb * k; w += k;
  } }
  // (the sky through a window is soft, and its few samples for a cell near the pane are noisy: the narrow passes take that out too)
  let sky_ = select(c.sky, vec4<f32>(dsky / w, 0.0), st <= 2);
  nxt[t.e] = CellV(c.sun, sky_, c.lamp, vec4<f32>(a / w, 0.0), vec4<f32>(e / w, 0.0), vec4<f32>(f / w, 0.0));
}
fn enc_e(c0: vec3<f32>) -> u32 {
  let c = clamp(c0, vec3<f32>(0.0), vec3<f32>(60000.0));
  let m = max(c.x, max(c.y, c.z));
  if (m < 1e-7) { return 0u; }
  var e = max(i32(floor(log2(m))), -16) + 1;
  var sc = ldexp(1.0, e - 9);
  if (floor(m / sc + 0.5) >= 512.0) { e += 1; sc *= 2.0; }
  let r = vec3<u32>(c / sc + vec3<f32>(0.5));
  return min(r.x, 511u) | (min(r.y, 511u) << 9u) | (min(r.z, 511u) << 18u) | (u32(e + 15) << 27u);
}
fn dec_e(v: u32) -> vec3<f32> {
  let sc = ldexp(1.0, i32(v >> 27u) - 15 - 9);
  return vec3<f32>(f32(v & 511u), f32((v >> 9u) & 511u), f32((v >> 18u) & 511u)) * sc;
}
// the channels, at the brightness asked for, packed in the table a frame looks up. Brightness changes by running this again
@compute @workgroup_size(64) fn bk_mix(@builtin(global_invocation_id) id: vec3<u32>) {
  let s = id.x + b.run.y; if (s >= b.run.z) { return; }
  let t = table[s];
  if (t.key == EMPTY) { out_tbl[s] = t; return; }
  let c = cur[t.e];
  let m = lm[t.e];
  let e0 = c.i_lamp.rgb * b.scales.x + (c.sky.rgb + c.i_sky.rgb) * b.scales.y + c.i_sun.rgb * b.scales.z;
  out_tbl[s] = Slot(t.key, enc_e(e0), c.sun.a, t.id, m.x, m.y, enc_e(e0 + c.lamp.rgb * b.scales.x), 0u);
}

// ---- a frame
var<private> bk_acc: vec3<f32>; var<private> bk_vis: f32; var<private> bk_w: f32; var<private> bk_any: u32; var<private> bk_all: u32;
fn add_layer(l: u32, side: u32, id: u32, ic: vec3<i32>, ax: u32, au: u32, av: u32, fu: f32, fv: f32, wl: f32) {
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    var c = ic; c[au] += i; c[av] += j;
    let s = find(key_of(l, c, side), id);
    if (s < 0) { continue; }
    let w = select(1.0 - fu, fu, i == 1) * select(1.0 - fv, fv, j == 1) * wl + 1e-4;
    let t = table[s];
    bk_acc += dec_e(t.e) * w; bk_vis += t.vis * w; bk_w += w; bk_any |= t.any; bk_all &= t.all;
  } }
}
// light at a point: between the four cells around it on its side, in its layer. A tilted surface crosses layers, and takes the two
fn lookup(q: vec3<f32>, n: vec3<f32>, id: u32) -> vec4<f32> {
  bk_acc = vec3<f32>(0.0); bk_vis = 0.0; bk_w = 0.0; bk_any = 0u; bk_all = 0xffffffffu;
  let l = level_of(q); let c = cell_of(l); let o = org_of(l);
  let sd = side_of(n); let ax = sd / 2u; let au = (ax + 1u) % 3u; let av = (ax + 2u) % 3u;
  let f = (q - o) / c - vec3<f32>(0.5);
  var ic = vec3<i32>(floor(f)); let fr = f - floor(f);
  let dir = select(1, -1, (sd & 1u) == 1u);
  if (abs(n[ax]) < 0.999) {
    add_layer(l, sd, id, ic, ax, au, av, fr[au], fr[av], 1.0 - fr[ax]);
    var c2 = ic; c2[ax] += 1;
    add_layer(l, sd, id, c2, ax, au, av, fr[au], fr[av], fr[ax]);
  } else {
    var c2 = ic; c2[ax] = i32(floor((q - o)[ax] / c));
    add_layer(l, sd, id, c2, ax, au, av, fr[au], fr[av], 1.0);
    if (bk_w <= 0.0) { c2[ax] -= dir; add_layer(l, sd, id, c2, ax, au, av, fr[au], fr[av], 1.0); }
    if (bk_w <= 0.0) { c2[ax] += 2 * dir; add_layer(l, sd, id, c2, ax, au, av, fr[au], fr[av], 1.0); }
  }
  if (bk_w <= 0.0) {
    // (nothing on this side: the wider search, then the next side round)
    for (var k = 0; k < 4 && bk_w <= 0.0; k++) {
      let ofs = array<vec2<i32>, 4>(vec2<i32>(-1, -1), vec2<i32>(1, -1), vec2<i32>(-1, 1), vec2<i32>(1, 1))[k];
      var c3 = ic; c3[ax] = i32(floor((q - o)[ax] / c)); c3[au] += ofs.x; c3[av] += ofs.y;
      add_layer(l, sd, id, c3, ax, au, av, 0.5, 0.5, 1.0);
    }
    if (bk_w <= 0.0) { let an = abs(n); var nn = n; nn[ax] = 0.0; let s2 = cell_at(q, nn + 1e-4, id); if (s2 >= 0) { let t = table[s2]; bk_acc = dec_e(t.e); bk_vis = t.vis; bk_w = 1.0; bk_any = t.any; bk_all = t.all; } }
    if (bk_w <= 0.0) { bk_any = 0xffffffffu; bk_all = 0u; return vec4<f32>(0.0, 0.0, 0.0, -1.0); }
  }
  return vec4<f32>(bk_acc / bk_w, bk_vis / bk_w);
}

// the lamps at this very point, without noise: each one it may see, by a ray only if its cell is in two minds
fn lamps_here(pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
  let nl = p.misc.z; var o = vec3<f32>(0.0); var have_o = false; var e = vec3<f32>(0.0);
  for (var l = 0u; l < nl; l++) {
    var trace = true;
    if (l < 32u) { let bit = 1u << l; if ((bk_any & bit) == 0u) { continue; } trace = (bk_all & bit) == 0u; }
    let v = lights[l].pos.xyz - pos; let r2 = max(dot(v, v), 0.04); let d = v * inverseSqrt(r2);
    let c = dot(n, d); if (c <= 0.0) { continue; }
    if (trace) {
      if (!have_o) { o = start_at(pos, n); have_o = true; }
      let v2 = lights[l].pos.xyz - o;
      let lv = length(v2);
      if (occluded(o, v2 / lv, lv - 0.02)) { continue; }
    }
    e += lights[l].col.xyz * c / r2;
  }
  return e * b.scales.x;
}
@group(1) @binding(10) var f_mat: texture_2d<f32>;
@group(1) @binding(11) var f_emis: texture_2d<f32>;
@group(1) @binding(12) var f_spec: texture_2d<f32>;
@group(1) @binding(13) var f_mirror: texture_2d<f32>;
@group(1) @binding(14) var o_lit: texture_storage_2d<rgba16float, write>;
@group(1) @binding(15) var f_in: texture_2d<f32>;
@group(1) @binding(16) var o_out: texture_storage_2d<rgba16float, write>;
@group(1) @binding(17) var f_hist: texture_2d<f32>;
@group(1) @binding(18) var smp: sampler;
@group(1) @binding(19) var o_8: texture_storage_2d<rgba8unorm, write>;

// reflections: one ray for a low-resolution pixel on what is glossy, lit from the cache where it lands
@compute @workgroup_size(8, 8) fn bk_spec(@builtin(global_invocation_id) id: vec3<u32>) {
  let lw = i32(p.dims.x) / 2; let lh = i32(p.dims.y) / 2;
  let px = vec2<i32>(id.xy);
  if (px.x >= lw || px.y >= lh) { return; }
  let g = px * 2 + vec2<i32>(i32(b.frame.y), i32(b.frame.z));
  let dims = vec2<f32>(f32(p.dims.x), f32(p.dims.y));
  let d = textureLoad(g_depth, g, 0);
  if (d >= 1.0) { textureStore(o_out, px, vec4<f32>(0.0)); return; }
  let a = textureLoad(g_alb, g, 0); let rough = a.a; let nc = textureLoad(g_nrm, g, 0); let coat = nc.a;
  if (!(rough >= 0.001 && (rough < 0.03 || (coat < 0.25 && rough < 0.5)))) { textureStore(o_out, px, vec4<f32>(0.0, 0.0, 0.0, rough)); return; }
  let pos = world_of(g, d, dims, p.inv_vp);
  var n = nc.xyz; if (dot(n, n) < 0.25) { n = -normalize(pos - p.cam.xyz); } n = normalize(n);
  seed = u32(px.x) * 2654435761u ^ u32(px.y) * 40503u ^ (b.frame.x * 1103515245u); seed = seed * 747796405u + 2891336453u;
  let o = start_at(pos, n);
  let r0 = reflect(normalize(pos - p.cam.xyz), n);
  var sp = vec3<f32>(0.0);
  // (a sharp reflection needs one ray; a rough one is what shows grain)
  let rays = select(1u, 4u, rough > 0.03);
  for (var ri = 0u; ri < rays; ri++) {
  var rd = r0;
  if (rough > 0.03) { rd = cone(r0, rough * rough * 1.2); }
  if (dot(rd, n) <= 0.0) { rd = r0; }
  let hh = closest(o, rd, 1000.0);
  var c_ = vec3<f32>(0.0);
  if (hh.i < 0) {
    c_ = sky_at(rd) * b.scales.y;
    if (dot(rd, p.sun_dir.xyz) > cos(p.scale.w + rough * rough * 0.4)) { c_ += p.sun_col.rgb * b.scales.z * (1.0 / (2.0 * PI * (1.0 - cos(p.scale.w + rough * rough * 0.4)))); }
  } else {
    let t = tris[hh.i]; let m = mats[t.mat];
    var ng = normalize(cross(t.e1.xyz, t.e2.xyz)); if (dot(ng, rd) > 0.0) { ng = -ng; }
    let q = o + rd * hh.t + ng * 0.002;
    let s = cell_at(q, ng, t.mat % 2047u);
    var e = vec3<f32>(0.0); var vs = 0.0;
    if (s >= 0) { let tt = table[s]; e = dec_e(tt.e_hit); vs = tt.vis; }
    c_ = m.emissive.rgb + m.albedo.rgb * (p.sun_col.rgb * b.scales.z * max(dot(ng, p.sun_dir.xyz), 0.0) * vs / PI + e);
  }
  sp += min(c_, vec3<f32>(20.0));
  }
  sp /= f32(rays);
  textureStore(o_out, px, vec4<f32>(min(sp, vec3<f32>(20.0)), rough));
}
// the rough ones have few rays: smoothed along the surface
@compute @workgroup_size(8, 8) fn bk_specf(@builtin(global_invocation_id) id: vec3<u32>) {
  let lw = i32(p.dims.x) / 2; let lh = i32(p.dims.y) / 2;
  let px = vec2<i32>(id.xy);
  if (px.x >= lw || px.y >= lh) { return; }
  let c = textureLoad(f_in, px, 0);
  let stride = i32(b.fl.w);
  if (c.a < 0.08 || (stride > 1 && c.a < 0.25)) { textureStore(o_out, px, c); return; }
  let off = vec2<i32>(i32(b.frame.y), i32(b.frame.z));
  let dims = vec2<f32>(f32(p.dims.x), f32(p.dims.y));
  let g = px * 2 + off; let d0 = textureLoad(g_depth, g, 0);
  if (d0 >= 1.0) { textureStore(o_out, px, c); return; }
  let n0 = normalize(textureLoad(g_nrm, g, 0).xyz + vec3<f32>(1e-5)); let p0 = world_of(g, d0, dims, p.inv_vp);
  var s = vec3<f32>(0.0); var sw = 0.0;
  for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
    let q = px + vec2<i32>(i, j) * stride;
    if (q.x < 0 || q.y < 0 || q.x >= lw || q.y >= lh) { continue; }
    let gq = q * 2 + off; let dq = textureLoad(g_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq = normalize(textureLoad(g_nrm, gq, 0).xyz + vec3<f32>(1e-5));
    let wg = f32(2 - abs(i)) * f32(2 - abs(j)) * pow(max(dot(n0, nq), 0.0), 32.0) * exp(-abs(dot(n0, world_of(gq, dq, dims, p.inv_vp) - p0)) / 0.03);
    s += textureLoad(f_in, q, 0).rgb * wg; sw += wg;
  } }
  textureStore(o_out, px, vec4<f32>(select(c.rgb, s / max(sw, 1e-5), sw > 1e-5), c.a));
}
@compute @workgroup_size(8, 8) fn bk_compose(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px0 = vec2<i32>(id.xy) + vec2<i32>(b.run.xy);
  if (px0.x >= w || px0.y >= h) { return; }
  let dims = vec2<f32>(f32(w), f32(h));
  var px = px0;
  var d = textureLoad(g_depth, px, 0);
  // (a crack between two meshes, where what lies behind the wall came through as white dots: a pixel far behind its neighbours on two opposite sides takes one of them)
  let dist0 = select(length(world_of(px0, d, dims, p.inv_vp) - p.cam.xyz), 1e9, d >= 1.0);
  for (var k = 0; k < 4; k++) {
    let o = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1))[k];
    let a0 = clamp(px0 + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)); let b0 = clamp(px0 - o, vec2<i32>(0), vec2<i32>(w - 1, h - 1));
    let da = textureLoad(g_depth, a0, 0); let db = textureLoad(g_depth, b0, 0);
    if (da < 1.0 && db < 1.0 && length(world_of(a0, da, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06 && length(world_of(b0, db, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06) { px = a0; d = da; break; }
  }
  if (d >= 1.0) {
    textureStore(o_lit, px0, vec4<f32>(sky_at(normalize(world_of(px0, 0.9999, dims, p.inv_vp) - p.cam.xyz)) * b.scales.y, 1.0));
    return;
  }
  let pos = world_of(px, d, dims, p.inv_vp);
  let nr = textureLoad(g_nrm, px, 0);
  let n = select(-normalize(pos - p.cam.xyz), normalize(nr.xyz), dot(nr.xyz, nr.xyz) > 0.25);
  let a = textureLoad(g_alb, px, 0); let mt = textureLoad(f_mat, px, 0);
  // the light, from the cache
  let lk = lookup(pos, n, u32(mt.a - 0.5));
  let cv = lk.a;
  let e = lk.rgb + lamps_here(pos, n);
  // the sun at this very point; a ray only where the cells around disagree or it is not plain
  let sun = p.sun_dir.xyz;
  let ndl = max(dot(n, sun), 0.0);
  var v = 0.0;
  if (ndl > 0.0) {
    if (cv >= 0.999) { v = 1.0; }
    else if (cv > 0.0 || cv < -0.5) {
      seed = u32(px0.x) + u32(px0.y) * 4099u + b.frame.x * 65521u;
      seed = seed * 747796405u + 2891336453u; seed = ((seed >> ((seed >> 28u) + 4u)) ^ seed) * 277803737u; seed = (seed >> 22u) ^ seed;
      // (two rays; where they differ, on a shadow's soft edge, more: one ray a pixel left it in grain)
      let so = start_at(pos, n);
      let v1 = select(0.0, 1.0, !occluded(so, cone(sun, p.scale.w), 1000.0)); let v2 = select(0.0, 1.0, !occluded(so, cone(sun, p.scale.w), 1000.0));
      v = (v1 + v2) * 0.5;
      if (v1 != v2) { var acc_ = v1 + v2; for (var k = 0; k < 6; k++) { acc_ += select(0.0, 1.0, !occluded(so, cone(sun, p.scale.w), 1000.0)); } v = acc_ / 8.0; }
    }
  }
  let metal = mt.x; let rough = a.a; let coat = nr.a;
  let vdir = normalize(p.cam.xyz - pos);
  let lit = e + p.sun_col.rgb * b.scales.z * ndl * v / PI;
  let f0 = mix(vec3<f32>(0.04), a.rgb, metal);
  let nv = max(dot(n, vdir), 0.001);
  let r_ = rough * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
  let a004 = min(r_.x * r_.x, pow(2.0, -9.28 * nv)) * r_.x + r_.y;
  let ab = vec2<f32>(-1.04, 1.04) * a004 + r_.zw;
  let fr = f0 * ab.x + vec3<f32>(ab.y);
  let fc = coat * (0.04 + 0.96 * pow(1.0 - nv, 5.0));
  let h_ = normalize(sun + vdir);
  let r2 = max(rough * rough, 0.002); let ndh = max(dot(n, h_), 0.0);
  let dn = ndh * ndh * (r2 * r2 - 1.0) + 1.0;
  let ggx = r2 * r2 / (PI * dn * dn + 1e-6);
  let vf = 0.25 / max(mix(nv, 1.0, r2 * 0.5) * mix(ndl, 1.0, r2 * 0.5), 0.05);
  let spec_sun = (f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h_, vdir), 0.0), 5.0)) * min(ggx * vf, 40.0) * ndl * p.sun_col.rgb * b.scales.z * v;
  // reflections, as rt_comp.wgsl: what rays found for the truly glossy, the room's light for the rest; thin brushed metal soft
  var flat_here = true;
  if (metal >= 0.5 && rough >= 0.03) {
    var off_ = 0; let r3 = 3;
    for (var k = 0; k < 4; k++) {
      let o = array<vec2<i32>, 4>(vec2<i32>(r3, 0), vec2<i32>(-r3, 0), vec2<i32>(0, r3), vec2<i32>(0, -r3))[k];
      let nq = textureLoad(g_nrm, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).xyz;
      if (dot(n, nq) < 0.98 * length(nq)) { off_++; }
    }
    flat_here = off_ < 2;
  }
  let traced = rough < 0.03 || (coat < 0.25 && rough < 0.5 && flat_here);
  var s = e;
  if (b.frame.w == 2u && rough < 0.001) {
    // (a mirror in a mirror's own picture: the room's light)
  } else if (rough < 0.001) {
    s = textureSampleLevel(f_mirror, smp, (vec2<f32>(px) + 0.5) / dims, 0.0).rgb;
  } else if (traced) {
    let lw = i32(p.dims.x) / 2; let lh = i32(p.dims.y) / 2;
    let ofs = vec2<f32>(f32(b.frame.y), f32(b.frame.z));
    let f = (vec2<f32>(px) - ofs) / 2.0;
    let bq = vec2<i32>(floor(f));
    var ss_ = vec3<f32>(0.0); var tw = 0.0;
    for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
      let q = clamp(bq + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(lw - 1, lh - 1));
      let gq = q * 2 + vec2<i32>(i32(b.frame.y), i32(b.frame.z));
      let dq = textureLoad(g_depth, gq, 0);
      if (dq >= 1.0) { continue; }
      let nq = textureLoad(g_nrm, gq, 0).xyz;
      let bw = (1.0 - abs(f.x - f32(bq.x + i))) * (1.0 - abs(f.y - f32(bq.y + j)));
      let wt = bw * pow(max(dot(n, normalize(nq + vec3<f32>(1e-5))), 0.0), 8.0) * exp(-abs(dot(n, world_of(gq, dq, dims, p.inv_vp) - pos)) / 0.02) + 1e-4 * bw;
      ss_ += textureLoad(f_spec, q, 0).rgb * wt; tw += wt;
    } }
    if (tw > 1e-3) { s = ss_ / tw; }
  }
  let refl = s * (fr + vec3<f32>(fc));
  let sheen = mt.z * (0.2 + 0.6 * pow(1.0 - nv, 4.0));
  let col = textureLoad(f_emis, px, 0).rgb + a.rgb * (1.0 - metal) * lit * (1.0 - fc) * (1.0 - sheen) + lit * sheen + refl + spec_sun;
  let show = p.misc2.w;
  if (show == 5u) { textureStore(o_lit, px0, vec4<f32>(vec3<f32>(v), 1.0)); return; }
  if (show == 6u) { textureStore(o_lit, px0, vec4<f32>(n * 0.5 + 0.5, 1.0)); return; }
  if (show == 2u) { textureStore(o_lit, px0, vec4<f32>(a.rgb, 1.0)); return; }
  if (show == 3u) { textureStore(o_lit, px0, vec4<f32>(lit, 1.0)); return; }
  if (show == 4u) { textureStore(o_lit, px0, vec4<f32>(refl + spec_sun, 1.0)); return; }
  if (show == 7u) { textureStore(o_lit, px0, vec4<f32>(select(0.0, 3.0, cv < -0.5), select(0.0, 1.0, traced), select(0.0, 1.0, cv > 0.0 && cv < 0.999), 1.0)); return; }
  textureStore(o_lit, px0, vec4<f32>(col, 1.0));
}

// AgX with its look, as shader.wgsl's agx
fn bk_agx(c: vec3<f32>) -> vec3<f32> {
  let ins = mat3x3<f32>(vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051), vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336), vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104));
  let outs = mat3x3<f32>(vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438), vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241), vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116));
  var v = ins * max(c, vec3<f32>(1e-10));
  v = clamp((log2(v) + 12.47393) / 16.5, vec3<f32>(0.0), vec3<f32>(1.0));
  let x2 = v * v; let x4 = x2 * x2;
  v = 15.5 * x4 * x2 - 40.14 * x4 * v + 31.96 * x4 - 6.868 * x2 * v + 0.4298 * x2 + 0.1191 * v - 0.00232;
  v = pow(max(v, vec3<f32>(0.0)), vec3<f32>(b.hi.w));
  v = outs * v;
  return pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}
fn bk_srgb(c: vec3<f32>) -> vec3<f32> { return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308)); }
// display-referred, with the distance from the eye in alpha: history is kept in what is shown, as the final filter averages what is shown
@compute @workgroup_size(8, 8) fn bk_tone(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  textureStore(o_out, px, vec4<f32>(bk_agx(textureLoad(f_in, px, 0).rgb * b.fl.y), 1.0));
}
fn hist_at(uv: vec2<f32>, dims: vec2<f32>) -> vec3<f32> {
  // Catmull-Rom from five bilinear taps: a blurred history is what makes the picture soft in motion
  let sp = uv * dims; let c = floor(sp - 0.5) + 0.5; let f = sp - c; let f2 = f * f; let f3 = f2 * f;
  let w0 = -0.5 * f3 + f2 - 0.5 * f; let w1 = 1.5 * f3 - 2.5 * f2 + vec2<f32>(1.0); let w2 = -1.5 * f3 + 2.0 * f2 + 0.5 * f; let w3 = 0.5 * f3 - 0.5 * f2;
  let w12 = w1 + w2; let t12 = (c + w2 / w12) / dims; let t0 = (c - 1.0) / dims; let t3 = (c + 2.0) / dims;
  var o = vec3<f32>(0.0); var wt = 0.0;
  let add = array<vec3<f32>, 5>(vec3<f32>(t12.x, t0.y, w12.x * w0.y), vec3<f32>(t0.x, t12.y, w0.x * w12.y), vec3<f32>(t12.x, t12.y, w12.x * w12.y), vec3<f32>(t3.x, t12.y, w3.x * w12.y), vec3<f32>(t12.x, t3.y, w12.x * w3.y));
  for (var k = 0; k < 5; k++) { o += textureSampleLevel(f_hist, smp, add[k].xy, 0.0).rgb * add[k].z; wt += add[k].z; }
  return max(o / wt, vec3<f32>(0.0));
}
@compute @workgroup_size(8, 8) fn bk_taa(@builtin(global_invocation_id) id: vec3<u32>) {
  let w = i32(p.dims.x); let h = i32(p.dims.y); let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let dims = vec2<f32>(f32(w), f32(h));
  let cur_ = textureLoad(f_in, px, 0).rgb;
  let d = textureLoad(g_depth, px, 0);
  let pos = select(world_of(px, d, dims, p.inv_vp), p.cam.xyz + normalize(world_of(px, 0.9999, dims, p.inv_vp) - p.cam.xyz) * 1000.0, d >= 1.0);
  let dist = length(pos - p.cam.xyz);
  var al = b.fl.x;
  if (al >= 1.0) { textureStore(o_out, px, vec4<f32>(cur_, dist)); return; }
  let c = b.prev_vp * vec4<f32>(pos, 1.0);
  let uv = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
  if (c.w <= 0.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) { textureStore(o_out, px, vec4<f32>(cur_, dist)); return; }
  var h_ = hist_at(uv, dims);
  if (b.frame.w == 1u) {
    // what was seen there before, kept only as far as it could still be what is around this pixel now
    var m1 = vec3<f32>(0.0); var m2 = vec3<f32>(0.0);
    for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
      let t = textureLoad(f_in, clamp(px + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).rgb;
      m1 += t; m2 += t * t;
    } }
    m1 /= 9.0; m2 /= 9.0;
    let sg = sqrt(max(m2 - m1 * m1, vec3<f32>(0.0))) * 1.25;
    let dh = textureLoad(f_hist, clamp(vec2<i32>(uv * dims), vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).a;
    let expect = length(pos - b.prev_eye.xyz);
    if (abs(dh - expect) > 0.05 * expect + 0.03) { al = max(al, 0.5); }
    h_ = clamp(h_, m1 - sg, m1 + sg);
  }
  textureStore(o_out, px, vec4<f32>(mix(h_, cur_, al), dist));
}
// the output: a filter wider than a pixel, as Blender's is
@compute @workgroup_size(8, 8) fn bk_final(@builtin(global_invocation_id) id: vec3<u32>) {
  let od = textureDimensions(o_8); let ow = i32(od.x); let oh = i32(od.y); let px = vec2<i32>(id.xy);
  if (px.x >= ow || px.y >= oh) { return; }
  let dm = vec2<i32>(textureDimensions(f_in)); let sc = f32(dm.x) / f32(ow);
  let c = (vec2<f32>(px) + 0.5) * sc - 0.5;
  let r = sc * 1.17;
  let base = vec2<i32>(floor(c - r + 0.999));
  var acc = vec3<f32>(0.0); var tw = 0.0;
  let span = i32(ceil(2.0 * r));
  for (var j = 0; j <= span; j++) { for (var i = 0; i <= span; i++) {
    let q = base + vec2<i32>(i, j);
    let wx = 1.0 - abs(f32(q.x) - c.x) / r; let wy = 1.0 - abs(f32(q.y) - c.y) / r;
    if (wx <= 0.0 || wy <= 0.0) { continue; }
    acc += textureLoad(f_in, clamp(q, vec2<i32>(0), dm - vec2<i32>(1)), 0).rgb * wx * wy; tw += wx * wy;
  } }
  textureStore(o_8, px, vec4<f32>(bk_srgb(acc / tw), 1.0));
}
