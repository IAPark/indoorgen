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
// ---- denoise: a-trous, edges from the G-buffer
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<uniform> stp: vec4<u32>;
@group(0) @binding(2) var d_depth: texture_depth_2d;
@group(0) @binding(3) var d_nrm: texture_2d<f32>;
@group(0) @binding(4) var i_irr: texture_2d<f32>;
@group(0) @binding(5) var i_spec: texture_2d<f32>;
@group(0) @binding(6) var o_irr: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var o_spec: texture_storage_2d<rgba16float, write>;
@compute @workgroup_size(8, 8) fn denoise(@builtin(global_invocation_id) id: vec3<u32>) {
  let ss = i32(p.dims.z); let w = i32(p.dims.x); let h = i32(p.dims.y);
  let px = vec2<i32>(id.xy);
  if (px.x >= w || px.y >= h) { return; }
  let dims = vec2<f32>(f32(w * ss), f32(h * ss));
  let g = px * ss + vec2<i32>(ss / 2);
  let d0 = textureLoad(d_depth, g, 0);
  var c = textureLoad(i_irr, px, 0); let sc = textureLoad(i_spec, px, 0);
  // (first round: a traced pixel unlike all around it started inside a wall's corner, and saw the outside)
  var av = vec4<f32>(0.0);
  if (stp.y == 0u && stp.z == 0u) {
    var hi = 0.0; var cn = 0.0;
    for (var j = -1; j <= 1; j++) { for (var i = -1; i <= 1; i++) {
      if (i == 0 && j == 0) { continue; }
      let q = px + vec2<i32>(i, j);
      if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
      let t = textureLoad(i_irr, q, 0);
      hi = max(hi, dot(t.rgb, vec3<f32>(0.333))); av += t; cn += 1.0;
    } }
    if (cn > 0.0) {
      av /= cn;
      if (dot(c.rgb, vec3<f32>(0.333)) > hi * 1.3 + 0.02) { c = vec4<f32>(av.rgb, c.a); }
    }
  }
  if (d0 >= 1.0) { textureStore(o_irr, px, c); textureStore(o_spec, px, sc); return; }
  let n0 = normalize(textureLoad(d_nrm, g, 0).xyz + vec3<f32>(1e-5));
  let p0 = world_of(g, d0, dims, p.inv_vp);
  var kw = array<f32, 3>(0.375, 0.25, 0.0625);
  let stride = i32(stp.x); let it = stp.y;
  var e = vec3<f32>(0.0); var ew = 0.0; var s = vec3<f32>(0.0); var sw = 0.0;
  // (the light's own edges fade from the filter as it widens: first it takes the noise out, then the room's low frequencies)
  for (var j = -2; j <= 2; j++) { for (var i = -2; i <= 2; i++) {
    let q = px + vec2<i32>(i, j) * stride;
    if (q.x < 0 || q.y < 0 || q.x >= w || q.y >= h) { continue; }
    let gq = q * ss + vec2<i32>(ss / 2);
    let dq = textureLoad(d_depth, gq, 0);
    if (dq >= 1.0) { continue; }
    let nq = normalize(textureLoad(d_nrm, gq, 0).xyz + vec3<f32>(1e-5));
    let pq = world_of(gq, dq, dims, p.inv_vp);
    let kk = kw[abs(i)] * kw[abs(j)];
    let wn = pow(max(dot(n0, nq), 0.0), 32.0);
    let wp = exp(-abs(dot(n0, pq - p0)) / (0.03 + 0.012 * f32(stride)));
    // (the pixel itself as the first round corrected it, or a sliver alone on its surface stays as it was: a bright dot)
    let ci = select(textureLoad(i_irr, q, 0), c, i == 0 && j == 0);
    let wg = kk * wn * wp;
    // (no test on brightness: in a room lit through a window most paths find nothing, and such a test keeps exactly the noise it should take out. No clamp either: what it cut was the sun's bounce)
    e += ci.rgb * wg; ew += wg;
    let si = textureLoad(i_spec, q, 0);
    // (reflections on a little relief: its normals differ pixel to pixel while what they reflect does not. A looser test, or a brushed fridge stayed in grains)
    let ws = kk * pow(max(dot(n0, nq), 0.0), 6.0) * wp;
    s += si.rgb * ws; sw += ws;
  } }
  // (one round is either the light's or the reflections': they are traced in turn)
  let spec_only = stp.z == 1u;
  let ee = select(select(c.rgb, e / max(ew, 1e-5), ew > 1e-5), c.rgb, spec_only);
  // (a mirror's picture is no noise: only rough reflections are smoothed, more the rougher)
  let spec_rounds = select(0u, select(1u, 3u, sc.a > 0.15), sc.a > 0.08);
  let sv = select(sc.rgb, s / max(sw, 1e-5), spec_only && sw > 1e-5 && it < spec_rounds);
  // (a sliver, alone on its surface: nothing to average its few paths with, so one lucky path is a bright dot on an edge. No brighter than what is around it)
  let lone = stp.y == 0u && stp.z == 0u && ew < 0.35 && dot(av.rgb, av.rgb) > 0.0;
  textureStore(o_irr, px, vec4<f32>(select(ee, min(ee, av.rgb), lone), 0.0));
  textureStore(o_spec, px, vec4<f32>(sv, sc.a));
}
