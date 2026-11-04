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
  let metal = mt.x; let rough = a.a;
  let vdir = normalize(p.cam.xyz - pos);
  let sun = p.sun_dir.xyz;
  let ndl = max(dot(n, sun), 0.0);
  let lit = e + p.sun_col.rgb * ndl * v / PI;
  // specular: what the reflection rays found, by the share a rough surface returns (Karis's fit), and a clear coat's own Fresnel on top
  let f0 = mix(vec3<f32>(0.04), a.rgb, metal);
  let nv = max(dot(n, vdir), 0.001);
  let r_ = rough * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
  let a004 = min(r_.x * r_.x, pow(2.0, -9.28 * nv)) * r_.x + r_.y;
  let ab = vec2<f32>(-1.04, 1.04) * a004 + r_.zw;
  let fr = f0 * ab.x + vec3<f32>(ab.y);
  let coat = textureLoad(c_nrm, px, 0).a;
  let fc = coat * (0.04 + 0.96 * pow(1.0 - nv, 5.0));
  let h_ = normalize(sun + vdir);
  let r2 = max(rough * rough, 0.002); let ndh = max(dot(n, h_), 0.0);
  let dn = ndh * ndh * (r2 * r2 - 1.0) + 1.0;
  let ggx = r2 * r2 / (PI * dn * dn + 1e-6);
  let vf = 0.25 / max(mix(nv, 1.0, r2 * 0.5) * mix(ndl, 1.0, r2 * 0.5), 0.05);
  let spec_sun = (f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(h_, vdir), 0.0), 5.0)) * min(ggx * vf, 40.0) * ndl * p.sun_col.rgb * v;
  // (rays only for the truly glossy: anything rougher, or a coat over fine relief as Blender's surfaces have, gives back the room's light, blurred. Traced, floors came out in sharp glare)
  let traced = rough < 0.03 || (coat < 0.25 && rough < 0.5);
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
