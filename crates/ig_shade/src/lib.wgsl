// Blender-like procedural textures for the previewer. Octaves stop where they would go below a pixel (the fine ones are invisible and alias), at
// most OCT: a detail of 15 costs no more than a detail of 6.
const OCT = 6.0;
fn ig_hash(x: vec3<i32>) -> u32 {
  var h = (u32(x.x) * 73856093u) ^ (u32(x.y) * 19349663u) ^ (u32(x.z) * 83492791u);
  h = (h ^ (h >> 13u)) * 1274126177u;
  return h ^ (h >> 16u);
}
// (fBM value, the amplitudes' sum) over the octaves a pixel can show
fn ig_fbm(p0: vec3<f32>, detail: f32, rough: f32, lac: f32) -> vec2<f32> {
  let fw = max(length(dpdx(p0)), length(dpdy(p0)));
  let visible = log2(0.7 / max(fw, 1e-6)) / max(log2(lac), 0.1);
  let oct = clamp(min(detail, OCT), -1.0, visible);
  var sum = 0.0; var amp = 1.0; var norm = 0.0; var fr = 1.0;
  let n = i32(floor(oct));
  for (var i = 0; i <= n; i++) { sum += amp * perlin_noise_3d(p0 * fr); norm += amp; amp *= rough; fr *= lac; }
  let t = oct - floor(oct);
  if (t > 0.0 && oct >= 0.0) { sum += t * amp * perlin_noise_3d(p0 * fr); norm += t * amp; }
  return vec2<f32>(sum, max(norm, 1e-6));
}
fn ig_warp(q: vec3<f32>, dist: f32) -> vec3<f32> {
  if (dist == 0.0) { return q; }
  return q + dist * vec3<f32>(perlin_noise_3d(q + vec3<f32>(12.9, 45.3, 7.1)), perlin_noise_3d(q + vec3<f32>(83.1, 21.7, 61.3)), perlin_noise_3d(q + vec3<f32>(37.7, 95.1, 29.9)));
}
fn ig_noise(v: vec3<f32>, scale: f32, detail: f32, rough: f32, lac: f32, dist: f32, w: f32) -> f32 {
  let q = ig_warp(v * scale + w * vec3<f32>(17.3, -9.1, 5.7), dist);
  let r = ig_fbm(q, detail, rough, lac);
  return 0.5 * r.x / r.y + 0.5;
}
fn ig_noise_color(v: vec3<f32>, scale: f32, detail: f32, rough: f32, lac: f32, dist: f32, w: f32) -> vec3<f32> {
  let q = ig_warp(v * scale + w * vec3<f32>(17.3, -9.1, 5.7), dist);
  let a = ig_fbm(q, detail, rough, lac); let b = ig_fbm(q + vec3<f32>(31.4, 5.9, 2.6), detail, rough, lac); let c = ig_fbm(q + vec3<f32>(53.5, 89.7, 9.3), detail, rough, lac);
  return 0.5 * vec3<f32>(a.x / a.y, b.x / b.y, c.x / c.y) + 0.5;
}
fn ig_musgrave(v: vec3<f32>, scale: f32, detail: f32, rough: f32, lac: f32, dist: f32, w: f32) -> f32 {
  return ig_fbm(v * scale + w * vec3<f32>(17.3, -9.1, 5.7), detail, rough, lac).x;
}
fn ig_hash3(c: vec3<i32>) -> vec3<f32> {
  let a = ig_hash(c); let b = ig_hash(c + vec3<i32>(101, 0, 0)); let d = ig_hash(c + vec3<i32>(0, 211, 0));
  return vec3<f32>(f32(a & 65535u), f32(b & 65535u), f32(d & 65535u)) / 65535.0;
}
fn ig_voronoi_color(p: vec3<f32>) -> vec3<f32> {
  let f0 = floor(p); let i = vec3<i32>(f0); let f = p - f0;
  var best = 9.0; var cell = i;
  for (var z = -1; z <= 1; z++) { for (var y = -1; y <= 1; y++) { for (var x = -1; x <= 1; x++) {
    let o = vec3<i32>(x, y, z);
    let d = vec3<f32>(o) + ig_hash3(i + o) - f;
    let dd = dot(d, d);
    if (dd < best) { best = dd; cell = i + o; }
  } } }
  return ig_hash3(cell + vec3<i32>(7, 13, 29));
}
fn ig_rotate(v: vec3<f32>, r: vec3<f32>) -> vec3<f32> {
  let cx = cos(r.x); let sx = sin(r.x); let cy = cos(r.y); let sy = sin(r.y); let cz = cos(r.z); let sz = sin(r.z);
  let a = vec3<f32>(v.x, cx * v.y - sx * v.z, sx * v.y + cx * v.z);
  let b = vec3<f32>(cy * a.x + sy * a.z, a.y, -sy * a.x + cy * a.z);
  return vec3<f32>(cz * b.x - sz * b.y, sz * b.x + cz * b.y, b.z);
}
fn ig_hsv_adjust(c: vec3<f32>, s: f32, v: f32) -> vec3<f32> {
  let g = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
  return mix(vec3<f32>(g), c, s) * v;
}
fn smooth_lin(x: f32, a: f32, b: f32) -> f32 { return clamp((x - a) / max(b - a, 1e-5), 0.0, 1.0); }
// Blender's brick texture: (the brick's random grey, the mortar mask)
fn ig_brick(p: vec3<f32>, scale: f32, row: f32, width: f32, offset: f32, mortar: f32) -> vec2<f32> {
  let q = p * scale;
  let rn = floor(q.y / row);
  let odd = i32(rn) & 1;
  let x = q.x + select(0.0, offset * width, odd == 1);
  let bn = floor(x / width);
  let fx = x - bn * width; let fy = q.y - rn * row;
  let d = min(min(fx, width - fx), min(fy, row - fy));
  let w = fwidth(d) + 1e-5;
  return vec2<f32>(f32(ig_hash(vec3<i32>(i32(bn), i32(rn), 3)) & 65535u) / 65535.0, 1.0 - smoothstep(mortar - w, mortar + w, d));
}
fn ig_checker(p: vec3<f32>) -> f32 {
  let c = floor(p + 1e-4);
  return f32((i32(c.x) + i32(c.y) + i32(c.z)) & 1);
}
