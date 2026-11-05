import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

import { rulesInit, rulesErrors } from './rules.js';

const $ = (id) => document.getElementById(id);
const canvas = $('view');
const EMBED = window.__INDOORGEN || null; // set by the single-file packager

// ---------------------------------------------------------------- renderer / scene
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, powerPreference: 'high-performance', preserveDrawingBuffer: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 0.85;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFShadowMap;

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x8fa2bb);
const hemi = new THREE.HemisphereLight(0xcfe0ff, 0x8a7e70, 1.4);
scene.add(hemi);
const sun = new THREE.DirectionalLight(0xfff0d8, 3.2);
sun.castShadow = true;
sun.shadow.mapSize.set(4096, 4096);
sun.shadow.bias = -0.0004;
sun.shadow.normalBias = 0.03;
scene.add(sun, sun.target);
const SUN_AZ = 215, SUN_EL = 38; // degrees; same as tools/preview

const cam = new THREE.PerspectiveCamera(60, 1, 0.05, 200);
cam.position.set(4, 3, 4);
const orbit = new OrbitControls(cam, canvas);
orbit.enableDamping = true;
orbit.dampingFactor = 0.12;

let content = null;      // THREE.Group of the loaded scene
let ceilings = [];
let lampLights = [];
let bbox = new THREE.Box3(new THREE.Vector3(-3, 0, -3), new THREE.Vector3(3, 2.6, 3));
let presets = [];        // {name, pos, quat, fov}
let lastGlb = null;      // ArrayBuffer for download
let lastName = 'room.glb';
let sidecar = null;

// ---------------------------------------------------------------- scene loading
const loader = new GLTFLoader();

function sunDir() {
  const az = THREE.MathUtils.degToRad(SUN_AZ), el = THREE.MathUtils.degToRad(SUN_EL);
  // azimuth measured in the Blender XY plane (Z up), converted to three (x, z, -y)
  const bx = Math.cos(el) * Math.sin(az), by = Math.cos(el) * Math.cos(az), bz = Math.sin(el);
  return new THREE.Vector3(bx, bz, -by); // direction pointing toward the sun
}

function disposeContent() {
  if (!content) return;
  scene.remove(content);
  content.traverse((o) => {
    if (o.geometry) o.geometry.dispose();
    if (o.material) (Array.isArray(o.material) ? o.material : [o.material]).forEach((m) => { for (const k in m) if (m[k] && m[k].isTexture) m[k].dispose(); m.dispose(); });
  });
  content = null;
}

function classify(root) {
  ceilings = [];
  const lamps = [];
  let tris = 0, meshes = 0;
  root.traverse((o) => {
    const n = (o.name || '').toLowerCase();
    if (!o.isMesh) return;
    meshes++;
    tris += (o.geometry.index ? o.geometry.index.count : o.geometry.attributes.position.count) / 3;
    const mats = Array.isArray(o.material) ? o.material : [o.material];
    const isGlass = /glass|window.*pane|mirror/.test(n) && !/mirror_frame/.test(n);
    const isCeil = /ceiling/.test(n) && !/light|lamp/.test(n);
    const isWall = /\.wall|skirting|door.*frame|window\d+$/.test(n);
    o.castShadow = !isGlass && !isCeil;
    o.receiveShadow = true;
    for (const m of mats) {
      if (m.map) m.map.anisotropy = 4;
      m.side = THREE.DoubleSide;
      if (isWall || isCeil) m.shadowSide = THREE.DoubleSide;
      if (/mirror/.test(n) && !/frame/.test(n)) { m.metalness = 0.9; m.roughness = 0.08; }
      if (isGlass && !/mirror/.test(n)) {
        m.transparent = true; m.opacity = 0.18; m.depthWrite = false; m.roughness = 0.05;
      }
    }
    if (isCeil) ceilings.push(o);
    if (/lamp|ceilinglight|chandelier|pendant|sconce/.test(n) && !isGlass) lamps.push(o);
  });
  return { lamps, tris, meshes };
}

function placeLamps(lamps) {
  lampLights.forEach((l) => scene.remove(l));
  lampLights = [];
  // one light per distinct lamp root (nodes often have several meshes)
  const seen = [];
  const b = new THREE.Box3(), c = new THREE.Vector3();
  for (const o of lamps) {
    b.setFromObject(o);
    if (b.isEmpty()) continue;
    b.getCenter(c);
    if (seen.some((p) => p.distanceTo(c) < 0.25)) continue;
    seen.push(c.clone());
  }
  seen.slice(0, 24).forEach((p) => {
    const l = new THREE.PointLight(0xffd9a8, 2.2, 6, 1.6);
    l.position.copy(p);
    scene.add(l);
    lampLights.push(l);
  });
}

function fitScene() {
  bbox = new THREE.Box3();
  // (the house alone: walking height, the corners and top-down all come from it. With terrain and trees in it the camera went under the floor, and far off)
  const isOutdoor = (o) => { for (let p = o; p; p = p.parent) if (/^(outdoor|exterior)/i.test(p.name || '')) return true; return false; };
  const whole = new THREE.Box3();
  content.traverse((o) => { if (o.isMesh && !ceilings.includes(o)) { whole.expandByObject(o); if (!isOutdoor(o)) bbox.expandByObject(o); } });
  if (bbox.isEmpty()) bbox.copy(whole);
  if (bbox.isEmpty()) bbox.setFromObject(content);
  const size = bbox.getSize(new THREE.Vector3()), ctr = bbox.getCenter(new THREE.Vector3());
  // sun shadow frustum
  const r = size.length() * 0.55;
  const d = sunDir();
  sun.position.copy(ctr).addScaledVector(d, r * 1.5);
  sun.target.position.copy(ctr);
  const sc = sun.shadow.camera;
  sc.left = -r; sc.right = r; sc.top = r; sc.bottom = -r; sc.near = 0.1; sc.far = r * 3.2;
  sc.updateProjectionMatrix();
  sun.target.updateMatrixWorld();
  // (far enough for the distant terrain, and no further: depth precision is what keeps near surfaces apart)
  const wholeLen = whole.isEmpty() ? size.length() : whole.getSize(new THREE.Vector3()).length();
  cam.far = Math.max(200, wholeLen * 1.5);
  cam.updateProjectionMatrix();
}

function blenderMatToCamera(m4) {
  // Blender world (Z up) -> three world (Y up): (x,y,z) -> (x,z,-y)
  const M = new THREE.Matrix4().set(...m4.flat());
  const C = new THREE.Matrix4().set(1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1);
  M.premultiply(C);
  const p = new THREE.Vector3(), q = new THREE.Quaternion(), s = new THREE.Vector3();
  M.decompose(p, q, s);
  return { pos: p, quat: q };
}

function buildPresets() {
  presets = [];
  if (sidecar && sidecar.cameras && sidecar.cameras.length) {
    for (const c of sidecar.cameras) {
      const { pos, quat } = blenderMatToCamera(c.matrix);
      const hfov = 2 * Math.atan((c.sensor_width || 36) / 2 / c.lens);
      presets.push({ name: c.name + (c.room ? ' (' + c.room + ')' : ''), pos, quat, hfov });
    }
  }
  // auto: corner camera towards the room centre, four corners
  const size = bbox.getSize(new THREE.Vector3()), ctr = bbox.getCenter(new THREE.Vector3());
  const names = ['auto corner NE', 'auto corner SW', 'auto corner NW', 'auto corner SE'];
  [[1, 1], [-1, -1], [-1, 1], [1, -1]].forEach(([sx, sz], i) => {
    const eye = new THREE.Vector3(ctr.x + sx * size.x * 0.46, bbox.min.y + Math.min(2.2, size.y * 0.85), ctr.z + sz * size.z * 0.46);
    const tgt = new THREE.Vector3(ctr.x, bbox.min.y + 0.7, ctr.z);
    const m = new THREE.Matrix4().lookAt(eye, tgt, new THREE.Vector3(0, 1, 0));
    presets.push({ name: names[i], pos: eye, quat: new THREE.Quaternion().setFromRotationMatrix(m), hfov: THREE.MathUtils.degToRad(80) });
  });
  const sel = $('cams');
  sel.innerHTML = '';
  presets.forEach((p, i) => sel.add(new Option(p.name, i)));
}

function vfovFor(hfov) {
  const a = cam.aspect;
  return THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(hfov / 2) / a));
}

function applyPreset(i) {
  const p = presets[i];
  if (!p) return;
  setMode('orbit');
  cam.position.copy(p.pos);
  cam.quaternion.copy(p.quat);
  cam.fov = Math.min(95, vfovFor(p.hfov));
  cam.updateProjectionMatrix();
  const fwd = new THREE.Vector3(0, 0, -1).applyQuaternion(p.quat);
  // orbit target: a point along view direction ~3 m ahead
  orbit.target.copy(p.pos).addScaledVector(fwd, 3);
  orbit.update();
}

function topDown() {
  setMode('orbit');
  const size = bbox.getSize(new THREE.Vector3()), ctr = bbox.getCenter(new THREE.Vector3());
  const fov = 50;
  cam.fov = fov;
  cam.updateProjectionMatrix();
  const half = Math.max(size.z / 2, size.x / 2 / cam.aspect) * 1.1;
  const h = half / Math.tan(THREE.MathUtils.degToRad(fov / 2));
  cam.position.set(ctr.x, bbox.min.y + h, ctr.z + 0.001);
  orbit.target.set(ctr.x, bbox.min.y, ctr.z);
  cam.lookAt(orbit.target);
  setCeiling(false);
  orbit.update();
}

async function showGlb(buf, name, sc) {
  if (typeof forgetHouse === 'function') forgetHouse();
  const gltf = await new Promise((res, rej) => loader.parse(buf.slice(0), '', res, rej));
  disposeContent();
  content = gltf.scene;
  sidecar = sc || null;
  const { lamps, tris, meshes } = classify(content);
  scene.add(content);
  content.updateMatrixWorld(true);
  placeLamps(lamps);
  fitScene();
  buildPresets();
  setCeiling(!hideCeil && true);
  applyPreset(0);
  lastGlb = buf; lastName = name; $('dl').disabled = false;
  $('info').textContent = `${name}\n${meshes} meshes, ${Math.round(tris / 1000)}k tris, ${lampLights.length} lamps, ${(buf.byteLength / 1e6).toFixed(1)} MB`;
  return { tris, meshes };
}

// ---------------------------------------------------------------- ceiling / modes
let hideCeil = false;
function setCeiling(show) {
  hideCeil = !show;
  ceilings.forEach((o) => (o.visible = show));
  $('ceil').textContent = show ? 'Hide ceiling' : 'Show ceiling';
  $('ceil').classList.toggle('on', !show);
}
$('ceil').onclick = () => setCeiling(hideCeil);
$('top').onclick = topDown;
$('cams').onchange = (e) => applyPreset(+e.target.value);

let mode = 'orbit';
const keys = new Set();
let yaw = 0, pitch = 0;
function setMode(m) {
  if (m === mode) return;
  mode = m;
  $('walk').classList.toggle('on', m === 'walk');
  orbit.enabled = m === 'orbit';
  $('help').style.display = '';
  if (m === 'walk') {
    const c = bbox.getCenter(new THREE.Vector3());
    const e = new THREE.Euler().setFromQuaternion(cam.quaternion, 'YXZ');
    yaw = e.y; pitch = 0;
    // start from the current camera position if it is inside the scene, else the room centre
    const inside = bbox.containsPoint(new THREE.Vector3(cam.position.x, bbox.min.y + 1, cam.position.z));
    if (!inside) { cam.position.x = c.x; cam.position.z = c.z; }
    cam.position.y = bbox.min.y + 1.6;
    cam.fov = 70; cam.updateProjectionMatrix();
    cam.rotation.set(pitch, yaw, 0, 'YXZ');
    if (canvas.requestPointerLock) { try { const r = canvas.requestPointerLock(); if (r && r.catch) r.catch(() => {}); } catch (e) {} }
  } else {
    if (document.pointerLockElement) document.exitPointerLock();
    const fwd = new THREE.Vector3(0, 0, -1).applyQuaternion(cam.quaternion);
    orbit.target.copy(cam.position).addScaledVector(fwd, 3);
    orbit.update();
  }
}
$('walk').onclick = () => setMode(mode === 'walk' ? 'orbit' : 'walk');
canvas.addEventListener('click', () => { if (mode === 'walk' && !document.pointerLockElement && canvas.requestPointerLock) canvas.requestPointerLock(); });
document.addEventListener('pointerlockchange', () => { if (mode === 'walk' && !document.pointerLockElement && false) setMode('orbit'); });
window.addEventListener('mousemove', (e) => {
  if (mode !== 'walk') return;
  if (!document.pointerLockElement && e.buttons !== 1) return; // fall back to drag-look
  yaw -= e.movementX * 0.0022; pitch -= e.movementY * 0.0022;
  pitch = Math.max(-1.45, Math.min(1.45, pitch));
});
window.addEventListener('keydown', (e) => {
  if (/INPUT|SELECT|TEXTAREA/.test(document.activeElement.tagName) && document.activeElement.type !== 'range') return;
  if (e.code === 'Escape' && mode === 'walk') setMode('orbit');
  keys.add(e.code);
});
window.addEventListener('keyup', (e) => keys.delete(e.code));
window.addEventListener('blur', () => keys.clear());

function walkStep(dt) {
  const sp = (keys.has('ShiftLeft') ? 3.2 : 1.6) * dt;
  const f = new THREE.Vector3(-Math.sin(yaw), 0, -Math.cos(yaw)), r = new THREE.Vector3(Math.cos(yaw), 0, -Math.sin(yaw));
  if (keys.has('KeyW') || keys.has('ArrowUp')) cam.position.addScaledVector(f, sp);
  if (keys.has('KeyS') || keys.has('ArrowDown')) cam.position.addScaledVector(f, -sp);
  if (keys.has('KeyD')) cam.position.addScaledVector(r, sp);
  if (keys.has('KeyA')) cam.position.addScaledVector(r, -sp);
  if (keys.has('ArrowLeft')) yaw += 1.5 * dt;
  if (keys.has('ArrowRight')) yaw -= 1.5 * dt;
  if (keys.has('KeyQ')) pitch = Math.min(1.45, pitch + dt);
  if (keys.has('KeyE')) pitch = Math.max(-1.45, pitch - dt);
  cam.position.y = bbox.min.y + 1.6;
  cam.rotation.set(pitch, yaw, 0, 'YXZ');
}

// ---------------------------------------------------------------- generation worker
let worker = null, workerReady = false, workerErr = null, jobId = 0, t0 = 0, tick = null;
const setStatus = (t, err) => { $('status').textContent = t; $('status').classList.toggle('err', !!err); };

function b64ToBytes(s) { const bin = atob(s); const u = new Uint8Array(bin.length); for (let i = 0; i < bin.length; i++) u[i] = bin.charCodeAt(i); return u; }

async function startWorker() {
  try {
    let w, wasm;
    if (EMBED) {
      w = new Worker(URL.createObjectURL(new Blob([b64ToBytes(EMBED.worker)], { type: 'text/javascript' })));
      wasm = b64ToBytes(EMBED.wasm).buffer;
    } else {
      w = new Worker('worker.js', { type: 'module' });
      // a gzipped module first (pkg/ig_wasm_gz.wasm, gzip bytes under a served extension: hosts with a per-file size limit; decompressed here unless the server already did), else the plain one
      wasm = null;
      try {
        const rz = await fetch('pkg/ig_wasm_gz.wasm');
        if (rz.ok) {
          const raw = new Uint8Array(await rz.arrayBuffer());
          if (raw[0] === 0x1f && raw[1] === 0x8b) {
            setStatus('unpacking wasm...');
            wasm = await new Response(new Blob([raw]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer();
          } else if (raw[0] === 0x00 && raw[1] === 0x61) wasm = raw.buffer;
        }
      } catch (_) { wasm = null; }
      if (!wasm) {
        const r = await fetch('pkg/ig_wasm_bg.wasm');
        if (!r.ok) throw new Error('pkg/ig_wasm_bg.wasm not found (wasm generator not built); use Load .glb');
        wasm = await r.arrayBuffer();
      }
    }
    worker = w;
    w.onmessage = onWorker;
    w.onerror = (e) => { workerErr = e.message || 'worker error'; setStatus('worker error: ' + workerErr, true); busy(false); };
    setStatus('loading wasm...');
    w.postMessage({ type: 'init', wasm }, [wasm]);
  } catch (e) {
    workerErr = String(e.message || e);
    setStatus(workerErr, true);
    $('gen').disabled = true;
  }
}

function busy(b) { $('gen').disabled = b || !workerReady; if (!b && tick) { clearInterval(tick); tick = null; } }

async function onWorker(ev) {
  const m = ev.data;
  if (m.type === 'ready') { workerReady = true; $('gen').disabled = false; setStatus('ready'); cfgInit(m.schema, m.defaults); { const u = new URLSearchParams(location.search); if (u.has('seed')) $('seed').value = u.get('seed'); if (u.has('room')) $('room').value = u.get('room'); if (u.has('auto')) generate(); } }
  else if (m.type === 'rulesinfo') rulesInit(m.info, window.IGConfig);
  else if (m.type === 'initfail') { workerErr = m.error; setStatus('wasm init failed: ' + m.error, true); }
  else if (m.type === 'status') setStatus(m.text);
  else if (m.type === 'fail') { setStatus('generation failed: ' + m.error, true); busy(false); }
  else if (m.type === 'done') {
    const genMs = m.ms;
    setStatus(`loading ${(m.glb.byteLength / 1e6).toFixed(1)} MB glb...`);
    try {
      const t1 = performance.now();
      const room = $('room').value, seed = $('seed').value;
      let sc = null;
      try { sc = m.scene ? (typeof m.scene === 'string' ? JSON.parse(m.scene) : m.scene) : null; } catch (_) {}
      await showGlb(m.glb, `${room}_${seed}.glb`, sc);
      const loadMs = performance.now() - t1;
      window.__lastTiming = { generate_ms: genMs, load_ms: loadMs, total_ms: performance.now() - t0 };
      setStatus(`done: generate ${(genMs / 1000).toFixed(2)} s + load ${(loadMs / 1000).toFixed(2)} s`);
    } catch (e) { setStatus('glb load failed: ' + e, true); }
    busy(false);
  }
}

// ---------------------------------------------------------------- generation settings
// The panel is generated from the JSON Schema of the Rust GenConfig (wasm config_schema()); defaults come from default_config().
// cfg = the full effective config object; presets are partial configs merged over the defaults; "extra JSON" is merged last.
const PRESETS = {
  fast: {},
  full: { solver: { steps_scale: 1.0 }, cloth: { steps: 2, cg_iters: 40 } },
  draft: { solver: { steps_scale: 0.25 }, cloth: { steps: 1, cg_iters: 10 }, mess: { level: 0.3 }, geometry: { budget_scale: 0.5 }, cameras: { count: 2 } },
};
const SKIP_SECTIONS = new Set(['rules', 'advanced', 'people', 'output', 'variation']); // house: only seed/room are hidden (controls above); rules: own editor; people/output: native only
const SKIP_FIELDS = new Set(['house.seed', 'house.room']);
let SCHEMA = null, DEFAULTS = {}, cfg = {};
const clone = (o) => JSON.parse(JSON.stringify(o));
const isObj = (v) => v && typeof v === 'object' && !Array.isArray(v);
function merge(base, over) { // deep merge, objects key by key, everything else replaces
  for (const [k, v] of Object.entries(over || {})) { if (isObj(v) && isObj(base[k])) merge(base[k], v); else base[k] = clone(v); }
  return base;
}
function diff(cur, def) { // minimal partial config: what differs from def
  const out = {};
  for (const [k, v] of Object.entries(cur || {})) {
    if (isObj(v) && isObj(def && def[k])) { const d = diff(v, def[k]); if (Object.keys(d).length) out[k] = d; }
    else if (JSON.stringify(v) !== JSON.stringify(def ? def[k] : undefined)) out[k] = v;
  }
  return out;
}
function getPath(o, path) { return path.split('.').reduce((x, k) => (x == null ? x : x[k]), o); }
function setPath(o, path, v) { const ks = path.split('.'); const last = ks.pop(); let x = o; for (const k of ks) { if (!isObj(x[k])) x[k] = {}; x = x[k]; } x[last] = v; }
function resolve(sch) { // follow $ref / single allOf
  if (!sch) return sch;
  if (sch.$ref) return resolve(SCHEMA.definitions[sch.$ref.split('/').pop()]);
  if (sch.allOf && sch.allOf.length === 1) return { ...resolve(sch.allOf[0]), ...(sch.description ? { description: sch.description } : {}) };
  return sch;
}
function typesOf(f) { const t = f.type; return Array.isArray(t) ? t : t ? [t] : []; }
const prettyName = (k) => k.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase());
let cfgPreset = 'fast';
let extraJson = '';
const cfgListeners = [];

function widget(path, f, onchange) {
  const ts = typesOf(f), nullable = ts.includes('null');
  const num = ts.includes('integer') || ts.includes('number'), isInt = ts.includes('integer');
  const v = getPath(cfg, path);
  let inp, out = null;
  if (ts.includes('boolean')) {
    inp = document.createElement('input'); inp.type = 'checkbox'; inp.checked = !!v; inp.onchange = () => onchange(inp.checked);
  } else if (f.enum) {
    inp = document.createElement('select'); for (const e of f.enum) inp.add(new Option(e, e)); inp.value = v; inp.onchange = () => onchange(inp.value);
  } else if (num) {
    const lo = f.minimum, hi = f.maximum;
    if (!isInt && lo != null && hi != null && !nullable && hi - lo <= 100 && !(lo > 0 && hi / lo > 1000)) { // bounded real: slider
      inp = document.createElement('input'); inp.type = 'range'; Object.assign(inp, { min: lo, max: hi, step: hi - lo >= 1 ? 0.05 : (hi - lo) / 100 }); inp.value = v;
      out = document.createElement('span'); out.className = 'v'; out.textContent = (+v).toFixed(3).replace(/\.?0+$/, '');
      inp.oninput = () => { out.textContent = (+inp.value).toFixed(3).replace(/\.?0+$/, ''); onchange(+inp.value); };
    } else {
      inp = document.createElement('input'); inp.type = 'number'; if (lo != null) inp.min = lo; if (hi != null) inp.max = hi; inp.step = isInt ? 1 : 'any';
      inp.value = v == null ? '' : v; if (nullable) inp.placeholder = 'auto';
      inp.onchange = () => onchange(inp.value === '' ? null : isInt ? Math.round(+inp.value) : +inp.value);
    }
  } else {
    inp = document.createElement('input'); inp.type = 'text'; inp.value = v == null ? '' : v; inp.placeholder = nullable ? 'none' : ''; inp.onchange = () => onchange(inp.value === '' ? null : inp.value);
  }
  return [inp, out];
}

function cfgRender() {
  const body = $('cfgbody'); body.innerHTML = '';
  if (!SCHEMA) { body.textContent = 'settings load with the wasm generator'; cfgShow(); return; }
  const root = resolve(SCHEMA);
  const ORDER = ['house', 'program', 'solver', 'mess', 'cloth', 'geometry', 'post', 'cameras'];
  const secs = Object.entries(root.properties).sort((a, b) => (ORDER.indexOf(a[0]) + 100) % 100 - (ORDER.indexOf(b[0]) + 100) % 100);
  for (const [sec, ssch] of secs) {
    if (SKIP_SECTIONS.has(sec)) continue;
    const sd = resolve(ssch);
    if (!sd.properties) continue;
    const det = document.createElement('details'); det.className = 'sec'; det.open = sec === 'solver';
    const sum = document.createElement('summary'); sum.textContent = prettyName(sec); if (sd.description) sum.title = sd.description; det.appendChild(sum);
    for (const [fk, fsch] of Object.entries(sd.properties)) {
      const path = sec + '.' + fk; if (SKIP_FIELDS.has(path)) continue;
      const f = resolve(fsch);
      const row = document.createElement('div'); row.className = 'row'; row.dataset.path = path;
      const tip = (f.description || '') + (f.minimum != null && f.maximum != null ? ` [${f.minimum} .. ${f.maximum}]` : '') + ` (${path})`;
      row.title = tip;
      const lab = document.createElement('label'); lab.textContent = prettyName(fk); row.appendChild(lab);
      const [inp, out] = widget(path, f, (nv) => cfgSet(path, nv));
      row.appendChild(inp); if (out) row.appendChild(out);
      det.appendChild(row);
    }
    body.appendChild(det);
  }
  cfgSync(); cfgShow();
}
// mirror the cfg values into the always-visible controls (clutter, cloth) and the generated rows' tooltips
function cfgSync() {
  const m = getPath(cfg, 'mess.level'), c = getPath(cfg, 'cloth.on'), od = getPath(cfg, 'program.outdoor.on');
  if (m != null) { $('mess').value = m; $('messv').textContent = (+m).toFixed(2); }
  if (c != null) $('cloth').checked = !!c;
  $('outdoors').checked = !!od;
  $('quality').value = cfgPreset;
}
function cfgSet(path, v, keepPreset) {
  setPath(cfg, path, v);
  if (!keepPreset) cfgPreset = 'custom';
  if (path === 'mess.level' || path === 'cloth.on' || path === 'program.outdoor.on') { cfgSync(); if (!keepPreset) { /* re-render so the generated row follows */ document.querySelectorAll(`#cfgbody [data-path="${path}"] input`).forEach((i) => { if (i.type === 'checkbox') i.checked = !!v; else { i.value = v; const o = i.parentNode.querySelector('.v'); if (o) o.textContent = (+v).toFixed(2); } }); } }
  cfgShow();
}
function applyCfgPreset(name) {
  cfgPreset = name;
  const keepRules = cfg.rules;
  cfg = merge(clone(DEFAULTS), PRESETS[name] || {});
  cfg.rules = keepRules ?? null; // presets do not touch the furniture rules
  cfgRender();
}
function extra() {
  const t = extraJson.trim(); if (!t) return null;
  try { const x = JSON.parse(t); $('cfgjson').style.borderColor = ''; return isObj(x) ? x : null; } catch (_) { $('cfgjson').style.borderColor = '#f98'; return null; }
}
// the config sent to generate(): the full effective config with the extra JSON merged last (legacy flat keys are accepted there too)
function buildOpts() { const o = clone(cfg); const x = extra(); if (x) merge(o, legacyToCfg(x)); return o; }
function legacyToCfg(x) { // old flat opts -> config sections (old shared links carried them)
  x = clone(x); const o = {};
  const put = (path, v) => setPath(o, path, v);
  if (typeof x.mess === 'number') { put('mess.level', x.mess); delete x.mess; }
  if (typeof x.cloth === 'boolean') { put('cloth.on', x.cloth); delete x.cloth; }
  if (typeof x.cams === 'number') { put('cameras.count', x.cams); delete x.cams; }
  for (const [k, p] of [['dense', 'program.dense'], ['only_target', 'house.only_target'], ['cloth_steps', 'cloth.steps'], ['cloth_cg', 'cloth.cg_iters'], ['steps_scale', 'solver.steps_scale']]) if (k in x) { put(p, x[k]); delete x[k]; }
  return merge(o, x);
}
function cfgShow() {
  $('cfgout').textContent = JSON.stringify(diff(buildOpts(), DEFAULTS));
  $('quality').value = cfgPreset;
  try { localStorage.setItem('indoorgen.cfg2', JSON.stringify({ cfg, j: extraJson, q: cfgPreset })); } catch (_) {}
  cfgListeners.forEach((fn) => { try { fn(cfg); } catch (_) {} });
}
$('quality').onchange = () => { if ($('quality').value !== 'custom') { applyCfgPreset($('quality').value); } else cfgPreset = 'custom'; };
$('cfgreset').onclick = () => { extraJson = ''; $('cfgjson').value = ''; applyCfgPreset(cfgPreset === 'custom' ? 'fast' : cfgPreset); };
$('cfgjson').oninput = () => { extraJson = $('cfgjson').value; cfgShow(); };
$('mess').addEventListener('input', () => cfgSet('mess.level', +$('mess').value));
$('cloth').addEventListener('change', () => cfgSet('cloth.on', $('cloth').checked));
$('outdoors').addEventListener('change', () => cfgSet('program.outdoor.on', $('outdoors').checked));
$('cfglink').onclick = async () => {
  const u = new URL(location.href); u.search = '';
  u.searchParams.set('seed', $('seed').value); u.searchParams.set('room', $('room').value);
  u.searchParams.set('cfg', btoa(JSON.stringify(diff(buildOpts(), DEFAULTS))));
  try { await navigator.clipboard.writeText(u.toString()); $('cfglink').textContent = 'Copied'; } catch (_) { prompt('Link', u.toString()); }
  setTimeout(() => ($('cfglink').textContent = 'Copy link'), 1500);
};
// called once the wasm module reported its schema + defaults: restore localStorage / URL state, build the panel
function cfgInit(schemaJson, defaultsJson) {
  try { SCHEMA = schemaJson ? JSON.parse(schemaJson) : null; DEFAULTS = defaultsJson ? JSON.parse(defaultsJson) : {}; } catch (e) { SCHEMA = null; }
  // (no terrain or trees unless asked for: they are most of a house's triangles here, and much of its time to generate)
  if (DEFAULTS.program && DEFAULTS.program.outdoor) DEFAULTS.program.outdoor.on = false;
  cfg = clone(DEFAULTS); cfgPreset = 'fast';
  try {
    const st = JSON.parse(localStorage.getItem('indoorgen.cfg2') || 'null');
    if (st && isObj(st.cfg)) { cfg = merge(clone(DEFAULTS), st.cfg); extraJson = st.j || ''; cfgPreset = st.q || 'custom'; }
  } catch (_) {}
  const u = new URLSearchParams(location.search);
  if (u.has('quality') && PRESETS[u.get('quality')]) { cfgPreset = u.get('quality'); cfg = merge(clone(DEFAULTS), PRESETS[cfgPreset]); extraJson = ''; }
  if (u.has('cfg')) { try { cfg = merge(clone(DEFAULTS), legacyToCfg(JSON.parse(atob(u.get('cfg'))))); extraJson = ''; cfgPreset = 'custom'; } catch (_) {} }
  if (u.has('mess')) { setPath(cfg, 'mess.level', +u.get('mess')); }
  $('cfgjson').value = extraJson;
  cfgRender();
}
// hook for the furniture-rules editor: it mounts into #rules-editor and reads/writes cfg.rules through this object
window.IGConfig = {
  get: () => clone(cfg), set: (path, v, keepPreset) => cfgSet(path, v, keepPreset), onChange: (fn) => cfgListeners.push(fn),
  get schema() { return SCHEMA; }, get defaults() { return clone(DEFAULTS); }, rulesMount: $('rules-editor'),
};

function generate() {
  if (!workerReady) return;
  if (rulesErrors().length) { $('cfg').open = true; $('rules-sec').open = true; setStatus('fix the furniture rules first: ' + rulesErrors()[0], true); return; }
  busy(true);
  t0 = performance.now();
  tick = setInterval(() => { $('status').dataset.t = ((performance.now() - t0) / 1000).toFixed(1); if (!$('status').classList.contains('err') && /\.\.\.$/.test($('status').textContent.replace(/ \(\d+\.\ds\)$/, ''))) $('status').textContent = $('status').textContent.replace(/ \(\d+\.\ds\)$/, '') + ` (${$('status').dataset.t}s)`; }, 200);
  worker.postMessage({
    type: 'gen', id: ++jobId, seed: +$('seed').value, room: $('room').value,
    opts: buildOpts(), // full GenConfig (seed/room are passed separately)
  });
}
$('gen').onclick = generate;
$('rand').onclick = () => { $('seed').value = Math.floor(Math.random() * 1e6); generate(); };
$('mess').oninput = () => ($('messv').textContent = (+$('mess').value).toFixed(2));
$('gen').disabled = true;

// ---------------------------------------------------------------- file in / out
$('file').onchange = async (e) => {
  const files = [...e.target.files];
  const glb = files.find((f) => /\.(glb|gltf)$/i.test(f.name));
  const js = files.find((f) => /\.json$/i.test(f.name));
  if (!glb) return;
  try {
    setStatus('loading ' + glb.name + '...');
    const t1 = performance.now();
    let sc = null;
    if (js) sc = JSON.parse(await js.text());
    await showGlb(await glb.arrayBuffer(), glb.name, sc);
    setStatus(`loaded in ${((performance.now() - t1) / 1000).toFixed(2)} s`);
  } catch (err) { setStatus('load failed: ' + err, true); }
};
$('dl').onclick = () => {
  if (!lastGlb) return;
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([lastGlb], { type: 'model/gltf-binary' }));
  a.download = lastName; a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
};

// optional: ?glb=url&json=url loads a scene directly (testing / sharing)
async function loadFromQuery() {
  const q = new URLSearchParams(location.search);
  if (!q.has('glb')) return;
  setStatus('loading ' + q.get('glb') + '...');
  const buf = await (await fetch(q.get('glb'))).arrayBuffer();
  let sc = null;
  if (q.has('json')) sc = await (await fetch(q.get('json'))).json();
  const r = await showGlb(buf, q.get('glb').split('/').pop(), sc);
  if (q.has('cam')) applyPreset(+q.get('cam'));
  if (q.has('top')) topDown();
  if (q.has('noceil')) setCeiling(false);
  setStatus('loaded');
  window.__sceneReady = true;
}

// ---------------------------------------------------------------- loop
function resize() {
  const w = window.innerWidth, h = window.innerHeight;
  renderer.setSize(w, h, false);
  cam.aspect = w / h;
  cam.updateProjectionMatrix();
}
window.addEventListener('resize', resize);
resize();

// ---------------------------------------------------------------- render this view (WebGPU): tools/preview's path tracer in the browser (tools/web_rt/build.sh)
const shot = $('renderview');
let shotKey = null, rendering = false, rtMod = null, rtReady = false;
function shotCamera(w, h) {
  // three (y up) to the house's own frame (z up): (x, y, z) -> (x, -z, y)
  const f = new THREE.Vector3(0, 0, -1).applyQuaternion(cam.quaternion);
  // (a camera looking straight down has no up: a little off it)
  if (Math.abs(f.y) > 0.999) { f.z -= 0.02; f.normalize(); }
  const p = cam.position;
  const et = new Float32Array([p.x, -p.z, p.y, p.x + f.x, -(p.z + f.z), p.y + f.y]);
  const tanX = Math.tan(THREE.MathUtils.degToRad(cam.fov / 2)) * (w / h);
  return { et, lens: 18 / tanX, key: Array.from(et).map((v) => v.toFixed(3)).join() };
}
function shotOff() {
  shotKey = null; shot.style.display = 'none';
  $('bakebtn').textContent = 'Render view';
}
function forgetHouse() { shotOff(); if (rtMod && rtReady) rtMod.forget(); }
async function renderView() {
  if (rendering) return;
  if (shotKey) { shotOff(); return; }
  if (!lastGlb) { setStatus('generate or load a house first'); return; }
  // (a page can offer WebGPU and still get no adapter, in a frame or on a blocked GPU)
  let adapter = null; try { adapter = navigator.gpu && await navigator.gpu.requestAdapter(); } catch (_) {}
  if (!adapter) { setStatus('Rendering needs WebGPU, which is not available here. Try opening the page in Chrome or Edge on its own.', true); return; }
  rendering = true; $('bakebtn').disabled = true; $('gen').disabled = true;
  try {
    setStatus('rendering: loading renderer...');
    rtMod = rtMod || await import('./pkg-rt/preview.js');
    if (!rtReady) { await rtMod.default(); rtReady = true; }
    // (the window's shape, about a million pixels: traced three times over each way, more would not fit many GPUs)
    const lim = Math.sqrt(1.0e6 / Math.max(1, innerWidth * innerHeight));
    const w = Math.max(320, Math.round(innerWidth * lim / 2) * 2), h = Math.max(180, Math.round(innerHeight * lim / 2) * 2);
    const c = shotCamera(w, h), t0 = performance.now();
    setStatus('rendering...');
    await new Promise((r) => requestAnimationFrame(r));
    const px = await rtMod.render_view(w, h, new Uint8Array(lastGlb), JSON.stringify(sidecar || {}), parseInt($('seed').value, 10) || 0, c.et, c.lens);
    shot.width = w; shot.height = h;
    shot.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(px), w, h), 0, 0);
    shot.style.display = 'block'; shotKey = c.key;
    $('bakebtn').textContent = 'Back to preview';
    setStatus(`rendered in ${((performance.now() - t0) / 1000).toFixed(1)} s. Move the camera to go back`);
  } catch (e) { setStatus('render failed: ' + (e.message || e), true); shotOff(); if (rtMod && rtReady) try { rtMod.forget(); } catch (_) {} }
  rendering = false; $('bakebtn').disabled = false; $('gen').disabled = !workerReady;
}
$('bakebtn').onclick = renderView;
if (!navigator.gpu) { $('bakebtn').disabled = true; $('bakebtn').title = 'Needs WebGPU, which this browser does not offer'; $('baknote').hidden = false; }

let last = performance.now();
renderer.setAnimationLoop(() => {
  const now = performance.now(), dt = Math.min(0.1, (now - last) / 1000);
  last = now;
  if (mode === 'walk') walkStep(dt); else orbit.update();
  // (nothing else on the GPU during a render)
  if (rendering) return;
  if (shotKey) { if (shotCamera(shot.width, shot.height).key === shotKey) return; shotOff(); }
  renderer.render(scene, cam);
});

window.__ig = { get sidecarJson() { return JSON.stringify(sidecar); }, renderView, get rendering() { return rendering; }, scene, cam, renderer, applyPreset, topDown, setCeiling, setMode, get presets() { return presets; }, generate };
loadFromQuery().catch((e) => setStatus('load failed: ' + e, true));
if (!new URLSearchParams(location.search).has('glb')) startWorker();

// menu toggle (index.html #menu): hides the panel; remembered per browser; M toggles it; on a phone the panel tucks away when a
// generation finishes so the room shows
(() => {
  const btn = document.getElementById('menu');
  if (!btn) return;
  const phone = () => window.matchMedia('(max-width:640px),(pointer:coarse) and (max-height:500px)').matches;
  const set = (hidden) => {
    document.body.classList.toggle('panel-hidden', hidden);
    btn.setAttribute('aria-expanded', String(!hidden));
    btn.innerHTML = hidden ? '&#x2630;' : '&#x2715;';
    try { localStorage.setItem('ig_panel_hidden', hidden ? '1' : '0'); } catch (_) {}
    window.dispatchEvent(new Event('resize'));
  };
  let start = false;
  try { start = localStorage.getItem('ig_panel_hidden') === '1'; } catch (_) {}
  set(start);
  btn.addEventListener('click', () => set(!document.body.classList.contains('panel-hidden')));
  window.addEventListener('keydown', (e) => {
    if ((e.key === 'm' || e.key === 'M') && !/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || '') && !document.pointerLockElement) set(!document.body.classList.contains('panel-hidden'));
  });
  const dl = document.getElementById('dl');
  if (dl) new MutationObserver(() => { if (!dl.disabled && phone()) set(true); }).observe(dl, { attributes: true, attributeFilter: ['disabled'] });
})();
