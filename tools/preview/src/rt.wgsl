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
struct W { c: vec4<f32>, u: vec4<f32>, v: vec4<f32>, n: vec4<f32> };
@group(0) @binding(11) var<storage, read> wins: array<W>;

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
        let t = tri_t(tris[n.a + k], o, d);
        if (t > 0.0005 && t < tmax) { return true; }
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
fn lamp(o: vec3<f32>, n: vec3<f32>) -> vec3<f32> { return lamp_rest(o, n, NO_LAMP, NO_LAMP); }
const NO_LAMP = 0xffffffu;
// one lamp, chosen by what it gives here, leaving out two
fn lamp_rest(o: vec3<f32>, n: vec3<f32>, ex0: u32, ex1: u32) -> vec3<f32> {
  let nl = p.misc.z; if (nl == 0u) { return vec3<f32>(0.0); }
  var total = 0.0; var i = 0u;
  loop { if (i >= nl) { break; }
    if (i != ex0 && i != ex1) {
      let v = lights[i].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
      total += dot(lights[i].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
    }
    i++; }
  if (total <= 0.0) { return vec3<f32>(0.0); }
  var pick = rnd() * total; i = 0u;
  loop { if (i >= nl) { break; }
    if (i != ex0 && i != ex1) {
      let v = lights[i].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
      let w = dot(lights[i].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
      if (pick < w) {
        if (occluded(o, l, sqrt(r2) - 0.02)) { return vec3<f32>(0.0); }
        return lights[i].col.xyz * max(dot(n, l), 0.0) / r2 * (total / w) * p.scale.x;
      }
      pick -= w;
    }
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
  for (var b = 0u; b <= bounces; b++) {
    let d = cosine(n);
    let h = closest(o, d, 1000.0);
    if (h.i < 0) { if (!through_window(o, d)) { e += tp * sky_at(d); } break; }
    let t = tris[h.i];
    let m = mats[t.mat];
    var ng = normalize(cross(t.e1.xyz, t.e2.xyz));
    if (dot(ng, d) > 0.0) { ng = -ng; }
    let q = o + d * h.t + ng * 0.002;
    e += tp * m.emissive.rgb;
    tp *= m.albedo.rgb;
    e += tp * direct(q, ng);
    o = q; n = ng;
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
  seed = u32(px.x) * 73856093u ^ u32(px.y) * 19349663u ^ (p.misc.y * 83492791u);
  let o = start_at(pos, n);
  // the two lamps that matter most here, apart from the filtered light, which would wash out their shadows
  var k0 = NO_LAMP; var k1 = NO_LAMP; var w0 = 0.0; var w1 = 0.0; var li = 0u;
  loop { if (li >= p.misc.z) { break; }
    let v = lights[li].pos.xyz - o; let r2 = max(dot(v, v), 0.04); let l = v * inverseSqrt(r2);
    let w_ = dot(lights[li].col.xyz, vec3<f32>(0.333)) * max(dot(n, l), 0.0) / r2;
    if (w_ > w0) { k1 = k0; w1 = w0; k0 = li; w0 = w_; } else if (w_ > w1) { k1 = li; w1 = w_; }
    li++; }
  var dl_ = vec3<f32>(0.0);
  if (k0 != NO_LAMP) {
    let v = lights[k0].pos.xyz - o; let r2 = max(dot(v, v), 0.04);
    dl_ += lights[k0].col.xyz * max(dot(n, v * inverseSqrt(r2)), 0.0) / r2 * p.scale.x * lamp_vis(o, lights[k0].pos.xyz);
  }
  if (k1 != NO_LAMP) {
    let v = lights[k1].pos.xyz - o; let r2 = max(dot(v, v), 0.04);
    dl_ += lights[k1].col.xyz * max(dot(n, v * inverseSqrt(r2)), 0.0) / r2 * p.scale.x * lamp_vis(o, lights[k1].pos.xyz);
  }
  textureStore(out_lamp, px, vec4<f32>(dl_, 1.0));
  var e = vec3<f32>(0.0);
  let spp = p.dims.w;
  for (var s = 0u; s < spp; s++) {
    let es = lamp_rest(o, n, k0, k1) + portal(o, n) + gather(o, n);
    // (only what cannot be real: a path that finds a sunlit patch is rare, valid, and most of a room's warm fill)
    e += es * min(1.0, 40.0 / max(max(max(es.x, es.y), es.z), 1e-5));
  }
  e /= f32(spp);
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
  if (rough < 0.001 || rough > 0.45 || p.misc2.w == 8u) { return vec3<f32>(0.0); }
  // relief finer than a pixel widens the highlight (Toksvig)
  var nb = n;
  for (var k = 0; k < 4; k++) {
    let dq = array<vec2<i32>, 4>(vec2<i32>(1, 1), vec2<i32>(-1, 1), vec2<i32>(1, -1), vec2<i32>(-1, -1))[k] * max(ss / 2, 1);
    let nq = textureLoad(g_nrm, clamp(g + dq, vec2<i32>(0), vec2<i32>(w * ss - 1, h * ss - 1)), 0).xyz;
    nb += select(n, nq, dot(nq, n) > 0.5);
  }
  nb /= 5.0;
  let nbl = max(length(nb), 0.01); let ns = nb / nbl;
  let a0 = sqrt(max(rough * rough * rough * rough + (1.0 - nbl) / nbl, 0.000004));
  let vdir = normalize(p.cam.xyz - pos); let nv = max(dot(ns, vdir), 0.001);
  // (the brightest lamp only: highlights rarely overlap, and one ray a pixel is what there is time for)
  var out = vec3<f32>(0.0); // (only where it would show: F0's 4 percent of it against a tenth of the light already here. Elsewhere a ray a pixel cost a quarter of the frame for nothing visible)
  let floor_ = max(dot(e, vec3<f32>(0.333)) * 0.1 / 0.04, 1.25); var best = floor_; var bl = vec3<f32>(0.0); var bd = 0.0; var i = 0u;
  loop { if (i >= p.misc.z) { break; }
    let v = lights[i].pos.xyz - pos; let r2 = max(dot(v, v), 0.04); let dl = sqrt(r2); let l = v / dl;
    let ndl = dot(ns, l);
    if (ndl > 0.0) {
      // a lamp is a small sphere: it widens the highlight, and spreads the same light (Karis)
      let a = min(a0 + LAMP_R / (2.0 * dl), 1.0); let keep = (a0 / a) * (a0 / a);
      let h_ = normalize(l + vdir); let ndh = max(dot(ns, h_), 0.0);
      let dn = ndh * ndh * (a * a - 1.0) + 1.0;
      let dv = keep * a * a / (PI * dn * dn + 1e-6) * 0.25 / max(mix(nv, 1.0, a * 0.5) * mix(ndl, 1.0, a * 0.5), 0.05);
      // (E / pi is what lamp() adds to the light; the sun's highlight has the same form)
      let c = lights[i].col.xyz * PI / r2 * p.scale.x * ndl * min(dv, 200.0);
      let m_ = dot(c, vec3<f32>(0.333));
      if (m_ > best) { best = m_; out = c; bl = l; bd = dl; }
    }
    i++; }
  if (bd <= 0.0 || occluded(o, bl, bd - 0.02)) { return vec3<f32>(0.0); }
  // (faded in from there, or the glow ended in a hard edge)
  return min(out, vec3<f32>(500.0)) * clamp(best / floor_ - 1.0, 0.0, 1.0);
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
  seed = u32(px.x) * 2654435761u ^ u32(px.y) * 40503u ^ (p.misc.y * 1103515245u);
  let o = start_at(pos, n);
  let e = textureLoad(irr_done, px, 0).rgb + textureLoad(lamp_done, px, 0).rgb;
  // what a glossy surface reflects
  let a = textureLoad(g_alb, g, 0);
  let rough = a.a;
  var sp = vec3<f32>(0.0);
  let coat = textureLoad(g_nrm, g, 0).a;
  if ((rough < 0.03 || (coat < 0.25 && rough < 0.5)) && p.misc2.y > 0u) {
    let v = normalize(pos - p.cam.xyz);
    let r0 = reflect(v, n);
    // (a flat mirror, alb's a 0: what it shows was traced already, as a picture from the camera mirrored in it, with textures and its own light)
    let is_mirror = rough < 0.001;
    let rays = select(p.misc2.y, 0u, is_mirror);
    if (is_mirror) { sp = textureLoad(m_lit, px, 0).rgb; }
    for (var s = 0u; s < rays; s++) {
      var rd = r0;
      if (rough > 0.03) { rd = cone(r0, min(rough * rough * 3.0, 0.8)); }
      if (dot(rd, n) <= 0.0) { rd = r0; }
      let hh = closest(o, rd, 1000.0);
      if (hh.i < 0) {
        var l = sky_at(rd);
        if (dot(rd, p.sun_dir.xyz) > cos(p.scale.w + rough * rough * 1.0)) { l += p.sun_col.rgb * (1.0 / (2.0 * PI * (1.0 - cos(p.scale.w + rough * rough * 1.0)))); }
        sp += l;
      } else {
        let t = tris[hh.i]; let m = mats[t.mat];
        var ng = normalize(cross(t.e1.xyz, t.e2.xyz));
        if (dot(ng, rd) > 0.0) { ng = -ng; }
        let q = o + rd * hh.t + ng * 0.002;
        // (the light it sees there, without the noise of more random choices: the sun, and the clean light of this pixel's own surroundings for the rest)
        // (a rough reflection, which the filter smooths: the light where the ray landed, by one path. In a shaded door frame this pixel's own light made what it reflects far too dark)
        var e_hit = e;
        if (rough > 0.15 && p.misc2.y > 1u) { // (not in the stripped mode)
          let eh = lamp(q, ng) + portal(q, ng) + gather(q, ng);
          e_hit = eh * min(1.0, 8.0 / max(max(max(eh.x, eh.y), eh.z), 1e-5));
        }
        sp += m.emissive.rgb + m.albedo.rgb * (p.sun_col.rgb * max(dot(ng, p.sun_dir.xyz), 0.0) * sun_vis(q, ng) / PI + e_hit);
      }
    }
    if (rays > 0u) { sp /= f32(rays); }
  }
  textureStore(out_lamp, px, vec4<f32>(lamp_smooth(px, pos, n, dims, w, h, ss), 1.0));
  textureStore(out_glint, px, vec4<f32>(lamp_glint(pos, n, o, g, rough, w, h, ss, e), 0.0));
  textureStore(out_spec, px, vec4<f32>(min(sp, vec3<f32>(20.0)), rough));
}
