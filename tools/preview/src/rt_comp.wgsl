struct P { inv_vp: mat4x4<f32>, cam: vec4<f32>, sun_dir: vec4<f32>, sun_col: vec4<f32>, dims: vec4<u32>, misc: vec4<u32>, misc2: vec4<u32>, scale: vec4<f32> };
const PI = 3.14159265;
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
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read> sky_t: array<vec4<f32>>;
@group(0) @binding(2) var c_depth: texture_depth_2d;
@group(0) @binding(3) var c_nrm: texture_2d<f32>;
@group(0) @binding(4) var c_alb: texture_2d<f32>;
@group(0) @binding(5) var c_mat: texture_2d<f32>;
@group(0) @binding(6) var c_irr: texture_2d<f32>;
@group(0) @binding(7) var c_spec: texture_2d<f32>;
@group(0) @binding(8) var c_emis: texture_2d<f32>;
@group(0) @binding(9) var o_lit: texture_storage_2d<rgba16float, write>;
@group(0) @binding(10) var c_fine: texture_2d<f32>;
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
const DFG = array<vec2<f32>, 256>(vec2<f32>(0.1430,0.8273),vec2<f32>(0.3852,0.6054),vec2<f32>(0.5694,0.4253),vec2<f32>(0.7065,0.2900),vec2<f32>(0.8062,0.1913),vec2<f32>(0.8767,0.1215),vec2<f32>(0.9249,0.0737),vec2<f32>(0.9566,0.0423),vec2<f32>(0.9765,0.0226),vec2<f32>(0.9883,0.0111),vec2<f32>(0.9947,0.0048),vec2<f32>(0.9979,0.0018),vec2<f32>(0.9992,0.0005),vec2<f32>(0.9997,0.0001),vec2<f32>(0.9999,0.0000),vec2<f32>(1.0000,0.0000),vec2<f32>(0.1376,0.6408),vec2<f32>(0.3646,0.5548),vec2<f32>(0.5490,0.4049),vec2<f32>(0.6884,0.2807),vec2<f32>(0.7907,0.1870),vec2<f32>(0.8638,0.1196),vec2<f32>(0.9143,0.0729),vec2<f32>(0.9480,0.0420),vec2<f32>(0.9696,0.0225),vec2<f32>(0.9828,0.0111),vec2<f32>(0.9905,0.0048),vec2<f32>(0.9947,0.0018),vec2<f32>(0.9969,0.0005),vec2<f32>(0.9982,0.0001),vec2<f32>(0.9990,0.0000),vec2<f32>(0.9997,0.0000),vec2<f32>(0.1861,0.4598),vec2<f32>(0.3485,0.4531),vec2<f32>(0.5195,0.3583),vec2<f32>(0.6576,0.2590),vec2<f32>(0.7623,0.1769),vec2<f32>(0.8389,0.1151),vec2<f32>(0.8933,0.0712),vec2<f32>(0.9306,0.0416),vec2<f32>(0.9554,0.0226),vec2<f32>(0.9714,0.0113),vec2<f32>(0.9816,0.0050),vec2<f32>(0.9879,0.0019),vec2<f32>(0.9920,0.0006),vec2<f32>(0.9948,0.0001),vec2<f32>(0.9968,0.0000),vec2<f32>(0.9986,0.0000),vec2<f32>(0.2803,0.3620),vec2<f32>(0.3621,0.3422),vec2<f32>(0.4976,0.2912),vec2<f32>(0.6228,0.2229),vec2<f32>(0.7249,0.1588),vec2<f32>(0.8037,0.1068),vec2<f32>(0.8621,0.0680),vec2<f32>(0.9039,0.0407),vec2<f32>(0.9331,0.0228),vec2<f32>(0.9534,0.0117),vec2<f32>(0.9672,0.0054),vec2<f32>(0.9766,0.0022),vec2<f32>(0.9833,0.0007),vec2<f32>(0.9882,0.0002),vec2<f32>(0.9922,0.0000),vec2<f32>(0.9956,0.0000),vec2<f32>(0.3863,0.2911),vec2<f32>(0.4061,0.2563),vec2<f32>(0.4959,0.2232),vec2<f32>(0.5964,0.1792),vec2<f32>(0.6875,0.1339),vec2<f32>(0.7628,0.0939),vec2<f32>(0.8222,0.0621),vec2<f32>(0.8675,0.0386),vec2<f32>(0.9015,0.0225),vec2<f32>(0.9264,0.0121),vec2<f32>(0.9448,0.0059),vec2<f32>(0.9582,0.0026),vec2<f32>(0.9684,0.0009),vec2<f32>(0.9767,0.0003),vec2<f32>(0.9833,0.0001),vec2<f32>(0.9888,0.0000),vec2<f32>(0.4835,0.2312),vec2<f32>(0.4646,0.1949),vec2<f32>(0.5149,0.1677),vec2<f32>(0.5851,0.1379),vec2<f32>(0.6574,0.1069),vec2<f32>(0.7232,0.0782),vec2<f32>(0.7792,0.0539),vec2<f32>(0.8253,0.0351),vec2<f32>(0.8618,0.0214),vec2<f32>(0.8906,0.0121),vec2<f32>(0.9128,0.0063),vec2<f32>(0.9304,0.0030),vec2<f32>(0.9446,0.0012),vec2<f32>(0.9563,0.0004),vec2<f32>(0.9659,0.0001),vec2<f32>(0.9744,0.0000),vec2<f32>(0.5630,0.1809),vec2<f32>(0.5243,0.1494),vec2<f32>(0.5444,0.1262),vec2<f32>(0.5876,0.1041),vec2<f32>(0.6389,0.0825),vec2<f32>(0.6908,0.0622),vec2<f32>(0.7388,0.0446),vec2<f32>(0.7810,0.0302),vec2<f32>(0.8170,0.0193),vec2<f32>(0.8470,0.0116),vec2<f32>(0.8722,0.0064),vec2<f32>(0.8931,0.0033),vec2<f32>(0.9107,0.0015),vec2<f32>(0.9259,0.0006),vec2<f32>(0.9392,0.0002),vec2<f32>(0.9509,0.0000),vec2<f32>(0.6228,0.1402),vec2<f32>(0.5757,0.1149),vec2<f32>(0.5756,0.0956),vec2<f32>(0.5976,0.0785),vec2<f32>(0.6301,0.0626),vec2<f32>(0.6666,0.0482),vec2<f32>(0.7035,0.0355),vec2<f32>(0.7384,0.0250),vec2<f32>(0.7704,0.0167),vec2<f32>(0.7988,0.0105),vec2<f32>(0.8244,0.0061),vec2<f32>(0.8470,0.0033),vec2<f32>(0.8667,0.0016),vec2<f32>(0.8843,0.0007),vec2<f32>(0.8999,0.0002),vec2<f32>(0.9138,0.0001),vec2<f32>(0.6635,0.1082),vec2<f32>(0.6147,0.0884),vec2<f32>(0.6023,0.0728),vec2<f32>(0.6090,0.0594),vec2<f32>(0.6259,0.0474),vec2<f32>(0.6483,0.0368),vec2<f32>(0.6735,0.0276),vec2<f32>(0.6996,0.0200),vec2<f32>(0.7255,0.0138),vec2<f32>(0.7503,0.0090),vec2<f32>(0.7731,0.0056),vec2<f32>(0.7942,0.0032),vec2<f32>(0.8136,0.0017),vec2<f32>(0.8316,0.0008),vec2<f32>(0.8477,0.0003),vec2<f32>(0.8621,0.0001),vec2<f32>(0.6873,0.0835),vec2<f32>(0.6406,0.0681),vec2<f32>(0.6210,0.0557),vec2<f32>(0.6167,0.0451),vec2<f32>(0.6217,0.0359),vec2<f32>(0.6328,0.0280),vec2<f32>(0.6475,0.0212),vec2<f32>(0.6649,0.0156),vec2<f32>(0.6828,0.0110),vec2<f32>(0.7010,0.0075),vec2<f32>(0.7190,0.0048),vec2<f32>(0.7365,0.0029),vec2<f32>(0.7530,0.0016),vec2<f32>(0.7686,0.0008),vec2<f32>(0.7835,0.0003),vec2<f32>(0.7979,0.0001),vec2<f32>(0.6968,0.0646),vec2<f32>(0.6536,0.0527),vec2<f32>(0.6299,0.0428),vec2<f32>(0.6183,0.0345),vec2<f32>(0.6148,0.0274),vec2<f32>(0.6168,0.0213),vec2<f32>(0.6230,0.0163),vec2<f32>(0.6313,0.0121),vec2<f32>(0.6413,0.0087),vec2<f32>(0.6523,0.0060),vec2<f32>(0.6639,0.0040),vec2<f32>(0.6757,0.0025),vec2<f32>(0.6875,0.0014),vec2<f32>(0.6998,0.0007),vec2<f32>(0.7118,0.0003),vec2<f32>(0.7237,0.0001),vec2<f32>(0.6941,0.0502),vec2<f32>(0.6546,0.0409),vec2<f32>(0.6289,0.0330),vec2<f32>(0.6124,0.0265),vec2<f32>(0.6027,0.0210),vec2<f32>(0.5981,0.0163),vec2<f32>(0.5964,0.0125),vec2<f32>(0.5971,0.0093),vec2<f32>(0.5997,0.0068),vec2<f32>(0.6035,0.0048),vec2<f32>(0.6082,0.0032),vec2<f32>(0.6142,0.0021),vec2<f32>(0.6210,0.0012),vec2<f32>(0.6282,0.0007),vec2<f32>(0.6359,0.0003),vec2<f32>(0.6437,0.0001),vec2<f32>(0.6814,0.0393),vec2<f32>(0.6452,0.0318),vec2<f32>(0.6185,0.0256),vec2<f32>(0.5988,0.0204),vec2<f32>(0.5848,0.0161),vec2<f32>(0.5743,0.0125),vec2<f32>(0.5665,0.0096),vec2<f32>(0.5609,0.0072),vec2<f32>(0.5568,0.0052),vec2<f32>(0.5542,0.0037),vec2<f32>(0.5533,0.0025),vec2<f32>(0.5535,0.0017),vec2<f32>(0.5546,0.0010),vec2<f32>(0.5567,0.0006),vec2<f32>(0.5594,0.0003),vec2<f32>(0.5621,0.0001),vec2<f32>(0.6606,0.0309),vec2<f32>(0.6270,0.0249),vec2<f32>(0.5998,0.0200),vec2<f32>(0.5781,0.0158),vec2<f32>(0.5604,0.0124),vec2<f32>(0.5456,0.0096),vec2<f32>(0.5330,0.0074),vec2<f32>(0.5221,0.0055),vec2<f32>(0.5130,0.0040),vec2<f32>(0.5057,0.0029),vec2<f32>(0.4995,0.0020),vec2<f32>(0.4946,0.0013),vec2<f32>(0.4906,0.0008),vec2<f32>(0.4875,0.0005),vec2<f32>(0.4848,0.0002),vec2<f32>(0.4824,0.0001),vec2<f32>(0.6335,0.0245),vec2<f32>(0.6016,0.0196),vec2<f32>(0.5743,0.0156),vec2<f32>(0.5511,0.0123),vec2<f32>(0.5305,0.0096),vec2<f32>(0.5123,0.0074),vec2<f32>(0.4960,0.0057),vec2<f32>(0.4814,0.0042),vec2<f32>(0.4687,0.0031),vec2<f32>(0.4572,0.0022),vec2<f32>(0.4471,0.0016),vec2<f32>(0.4381,0.0010),vec2<f32>(0.4299,0.0007),vec2<f32>(0.4223,0.0004),vec2<f32>(0.4151,0.0002),vec2<f32>(0.4086,0.0001),vec2<f32>(0.6017,0.0196),vec2<f32>(0.5709,0.0155),vec2<f32>(0.5433,0.0123),vec2<f32>(0.5188,0.0096),vec2<f32>(0.4962,0.0075),vec2<f32>(0.4756,0.0057),vec2<f32>(0.4566,0.0044),vec2<f32>(0.4396,0.0033),vec2<f32>(0.4240,0.0024),vec2<f32>(0.4097,0.0017),vec2<f32>(0.3967,0.0012),vec2<f32>(0.3844,0.0008),vec2<f32>(0.3729,0.0005),vec2<f32>(0.3620,0.0003),vec2<f32>(0.3519,0.0002),vec2<f32>(0.3425,0.0001));
fn dfg(nv: f32, rough: f32) -> vec2<f32> {
  var t = DFG;
  let x = clamp(nv * 16.0 - 0.5, 0.0, 15.0); let y = clamp(rough * 16.0 - 0.5, 0.0, 15.0);
  let i = u32(x); let j = u32(y); let i1 = min(i + 1u, 15u); let j1 = min(j + 1u, 15u); let fx = x - f32(i); let fy = y - f32(j);
  return mix(mix(t[j * 16u + i], t[j * 16u + i1], fx), mix(t[j1 * 16u + i], t[j1 * 16u + i1], fx), fy);
}
@compute @workgroup_size(8, 8) fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x) * ss; let h = i32(p.dims.y) * ss;
  let px0 = vec2<i32>(id.xy);
  if (px0.x >= w || px0.y >= h) { return; }
  let dims = vec2<f32>(f32(w), f32(h));
  var px = px0;
  var d = textureLoad(c_depth, px, 0);
  // (a crack between two meshes, where what lies behind the wall -- the sky, a sunlit outside -- came through as white dots: a pixel far behind its neighbours on two opposite sides takes one of them)
  let dist0 = select(length(world_of(px0, d, dims, p.inv_vp) - p.cam.xyz), 1e9, d >= 1.0);
  for (var k = 0; k < 8; k++) {
    let o = array<vec2<i32>, 8>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1), vec2<i32>(2, 0), vec2<i32>(0, 2), vec2<i32>(2, 2), vec2<i32>(2, -2))[k];
    let a0 = clamp(px0 + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)); let b0 = clamp(px0 - o, vec2<i32>(0), vec2<i32>(w - 1, h - 1));
    let da = textureLoad(c_depth, a0, 0); let db = textureLoad(c_depth, b0, 0);
    if (da < 1.0 && db < 1.0 && length(world_of(a0, da, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06 && length(world_of(b0, db, dims, p.inv_vp) - p.cam.xyz) < dist0 - 0.06) { px = a0; d = da; break; }
  }
  if (d >= 1.0) {
    let rd = normalize(world_of(px0, 0.9999, dims, p.inv_vp) - p.cam.xyz);
    // (a cutaway stands in no world: plain behind it)
    let bg = select(sky_at(rd), vec3<f32>(0.25), p.misc2.w == 1u);
    textureStore(o_lit, px0, vec4<f32>(bg, 1.0));
    return;
  }
  let pos = world_of(px, d, dims, p.inv_vp);
  let nr = textureLoad(c_nrm, px, 0).xyz;
  let n = select(-normalize(pos - p.cam.xyz), normalize(nr), dot(nr, nr) > 0.25);
  let a = textureLoad(c_alb, px, 0); let mt = textureLoad(c_mat, px, 0);
  // the filtered light, from the traced pixels around this one that lie on the same surface
  let lw = i32(p.dims.x); let lh = i32(p.dims.y);
  let f = (vec2<f32>(px) + 0.5) / f32(ss) - 0.5;
  let b = vec2<i32>(floor(f));
  var e = vec3<f32>(0.0); var s = vec3<f32>(0.0); var tw = 0.0;
  for (var j = 0; j < 2; j++) { for (var i = 0; i < 2; i++) {
    let q = clamp(b + vec2<i32>(i, j), vec2<i32>(0), vec2<i32>(lw - 1, lh - 1));
    let gq = q * ss + vec2<i32>(ss / 2);
    let dq = textureLoad(c_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let pq = world_of(gq, dq, dims, p.inv_vp);
    let nq = textureLoad(c_nrm, gq, 0).xyz;
    let bw = (1.0 - abs(f.x - f32(b.x + i))) * (1.0 - abs(f.y - f32(b.y + j)));
    let wt = bw * pow(max(dot(n, normalize(nq + vec3<f32>(1e-5))), 0.0), 8.0) * exp(-abs(dot(n, pq - pos)) / 0.02) + 1e-4 * bw;
    let c = textureLoad(c_irr, q, 0);
    e += c.rgb * wt; s += textureLoad(c_spec, q, 0).rgb * wt; tw += wt;
  } }
  e /= max(tw, 1e-5); s /= max(tw, 1e-5);
  // the sun at this very point, and the light of a surface no traced pixel landed on
  let fine = textureLoad(c_fine, px, 0);
  var v = fine.a;
  // (a shadow's soft edge, from few rays: smoothed along this surface, only where the sun's visibility changes nearby)
  var v_lo = v; var v_hi = v;
  for (var k = 0; k < 4; k++) {
    let o = array<vec2<i32>, 4>(vec2<i32>(2, 0), vec2<i32>(-2, 0), vec2<i32>(0, 2), vec2<i32>(0, -2))[k];
    let t = textureLoad(c_fine, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).a;
    v_lo = min(v_lo, t); v_hi = max(v_hi, t);
  }
  if (v_hi > v_lo) {
    var vs = 0.0; var vc = 0.0;
    for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
      let q = px + vec2<i32>(i, j);
      if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
      let dq = textureLoad(c_depth, q, 0);
      if (dq >= 1.0 || abs(dot(n, world_of(q, dq, dims, p.inv_vp) - pos)) > 0.01) { continue; }
      let wq = f32(3 - abs(i)) * f32(3 - abs(j));
      vs += textureLoad(c_fine, q, 0).a * wq; vc += wq;
    } }
    if (vc > 0.0) { v = vs / vc; }
  }
  if (fine.r >= 0.0) { e = min(fine.rgb, e * 1.2); }
  let metal = mt.x;
  // (cloth is matte, as render_rust.py makes it: very rough, a weak specular)
  let cloth = mt.z > 0.0;
  let rough = select(a.a, max(a.a, 0.85), cloth);
  let vdir = normalize(p.cam.xyz - pos);
  let sun = p.sun_dir.xyz;
  let ndl = max(dot(n, sun), 0.0);
  let lit = e + p.sun_col.rgb * ndl * v / PI;
  // specular: what the reflection rays found, by the share a rough surface returns (Karis's fit), and a clear coat's own Fresnel on top
  let f0 = mix(vec3<f32>(select(0.04, 0.02, cloth)), a.rgb, metal);
  let nv = max(dot(n, vdir), 0.001);
  // (a table for the share a rough surface returns, from integrating GGX: Karis's short fit gave too little at glancing angles, and dark cabinets seen from below came out too dark)
  let ab = dfg(nv, rough);
  let fr = f0 * ab.x + vec3<f32>(ab.y);
  let coat = textureLoad(c_nrm, px, 0).a;
  let fc = coat * (0.04 + 0.96 * pow(1.0 - nv, 5.0));
  // the sun's highlight, on the normal as this part of the picture sees it: relief finer than a pixel widens the highlight (Toksvig), where taken point by point it sparkled, and leather came out speckled
  var nb = n;
  for (var k = 0; k < 8; k++) {
    let o = array<vec2<i32>, 8>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1), vec2<i32>(2, 2), vec2<i32>(-2, 2), vec2<i32>(2, -2), vec2<i32>(-2, -2))[k];
    let nq = textureLoad(c_nrm, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).xyz;
    if (dot(nq, n) > 0.5) { nb += nq; } else { nb += n; }
  }
  nb /= 9.0;
  let nbl = max(length(nb), 0.01); let ns = nb / nbl;
  let h_ = normalize(sun + vdir);
  let r2 = sqrt(max(rough * rough * rough * rough + (1.0 - nbl) / nbl, 0.000004)); let ndh = max(dot(ns, h_), 0.0);
  let ndl_s = max(dot(ns, sun), 0.0);
  let dn = ndh * ndh * (r2 * r2 - 1.0) + 1.0;
  let ggx = r2 * r2 / (PI * dn * dn + 1e-6);
  let vf = 0.25 / max(mix(nv, 1.0, r2 * 0.5) * mix(ndl_s, 1.0, r2 * 0.5), 0.05);
  let spec_sun = (f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h_, vdir), 0.0), 5.0)) * min(ggx * vf, 40.0) * ndl_s * p.sun_col.rgb * v;
  // (rays only for the truly glossy: anything rougher, or a coat over fine relief as Blender's surfaces have, gives back the room's light, blurred. Traced, floors came out in sharp glare)
  // (and not on a thin rail no traced pixel landed on: what it would borrow is its neighbours', who traced nothing, and bright metal came out black)
  // (brushed metal that is thin or curved too: a sharp ray per pixel put hard glints on rails and frames that Blender, with its fine relief, draws soft. Flat panels keep their reflections)
  var flat_here = true;
  if (metal >= 0.5 && rough >= 0.03) {
    // (one side is only an edge nearby. Not in a mirror's picture, at one sample a pixel: there a speck of relief counted, and left dots across a door)
    var off_ = 0; let r_ = ss + 1;
    for (var k = 0; k < 4; k++) {
      let o = array<vec2<i32>, 4>(vec2<i32>(r_, 0), vec2<i32>(-r_, 0), vec2<i32>(0, r_), vec2<i32>(0, -r_))[k];
      let nq = textureLoad(c_nrm, clamp(px + o, vec2<i32>(0), vec2<i32>(w - 1, h - 1)), 0).xyz;
      if (dot(n, nq) < 0.98 * length(nq)) { off_++; }
    }
    flat_here = off_ < 2 || ss == 1;
  }
  let traced = (rough < 0.03 || (coat < 0.25 && rough < 0.5 && flat_here)) && fine.r < 0.0;
  var refl = select(e, s, traced) * (fr + vec3<f32>(fc));
  // cloth's sheen: fine fibres send back a share of the light uncoloured, more at grazing angles, taking it from the colour underneath
  let sheen = mt.z * (0.2 + 0.6 * pow(1.0 - nv, 4.0));
  let col = textureLoad(c_emis, px, 0).rgb + a.rgb * (1.0 - metal) * lit * (1.0 - fc) * (1.0 - sheen) + lit * sheen + refl + spec_sun;
  // (RT_SHOW: one ingredient of the picture alone, to find which is wrong)
  if (p.misc2.w == 7u) { textureStore(o_lit, px0, vec4<f32>(select(0.0, 3.0, fine.r >= 0.0), min(tw, 1.0) * 0.5, select(0.0, 3.0, any(px != px0)), 1.0)); return; }
  if (p.misc2.w == 5u) { textureStore(o_lit, px0, vec4<f32>(vec3<f32>(v), 1.0)); return; }
  if (p.misc2.w == 6u) { textureStore(o_lit, px0, vec4<f32>(n * 0.5 + 0.5, 1.0)); return; }
  if (p.misc2.w == 2u) { textureStore(o_lit, px0, vec4<f32>(a.rgb, 1.0)); return; }
  if (p.misc2.w == 3u) { textureStore(o_lit, px0, vec4<f32>(lit, 1.0)); return; }
  if (p.misc2.w == 4u) { textureStore(o_lit, px0, vec4<f32>(refl + spec_sun, 1.0)); return; }
  textureStore(o_lit, px0, vec4<f32>(col, 1.0));
}
