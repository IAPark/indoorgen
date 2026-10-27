import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

import { rulesInit, getRules, rulesErrors, rulesLinkParam } from './rules.js';

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
  content.traverse((o) => { if (o.isMesh && !ceilings.includes(o)) bbox.expandByObject(o); });
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
  cam.far = Math.max(200, size.length() * 6);
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
      const r = await fetch('pkg/ig_wasm_bg.wasm');
      if (!r.ok) throw new Error('pkg/ig_wasm_bg.wasm not found (wasm generator not built); use Load .glb');
      wasm = await r.arrayBuffer();
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
  if (m.type === 'ready') { workerReady = true; $('gen').disabled = false; setStatus('ready'); { const u = new URLSearchParams(location.search); if (u.has('seed')) $('seed').value = u.get('seed'); if (u.has('room')) $('room').value = u.get('room'); if (u.has('mess')) { $('mess').value = u.get('mess'); cfgVals.mess = +u.get('mess'); cfgShow(); } if (u.has('auto')) generate(); } }
  else if (m.type === 'rulesinfo') rulesInit(m.info);
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
// k: option key in generate(opts) or "env.NAME" for a native tunable; inv: checkbox ON means the env flag is NOT set
const CFG = [
  { grp: 'Furnishing' },
  { k: 'dense', label: 'Dense furniture rules', type: 'bool', def: true, tip: 'off = the program exactly as exported from Infinigen (fewer objects)' },
  { k: 'only_target', label: 'Only target room', type: 'bool', def: true, tip: 'off = furnish every room of the house (much slower)' },
  { k: 'env.TRINKETS_PER_SHELF', label: 'Trinkets / shelf', type: 'int', def: 3, min: 0, max: 20 },
  { k: 'env.TRINKETS_MAX', label: 'Trinkets / house', type: 'int', def: 40, min: 0, max: 400 },
  { k: 'env.NO_COUNTER_WIN', label: 'Counters may cover windows', type: 'bool', def: true, inv: true },
  { grp: 'Solver' },
  { k: 'steps_scale', label: 'Steps scale', type: 'range', def: 0.6, min: 0.1, max: 2, step: 0.05, tip: 'x 300/200/50 annealing steps (large/medium/small)' },
  { k: 'env.STEPS_L', label: 'Large steps (blank=scale)', type: 'int', def: null, min: 1, max: 2000 },
  { k: 'env.STEPS_M', label: 'Medium steps', type: 'int', def: null, min: 1, max: 2000 },
  { k: 'env.STEPS_S', label: 'Small steps', type: 'int', def: null, min: 1, max: 1000 },
  { k: 'env.PLACE_TRIES', label: 'Pose tries (big items)', type: 'int', def: 40, min: 1, max: 200 },
  { k: 'env.CONSTRUCTIVE', label: 'Constructive placement', type: 'bool', def: false },
  { grp: 'Clutter & cloth' },
  { k: 'mess', label: 'Clutter level', type: 'range', def: 0.5, min: 0, max: 1, step: 0.05, mirror: 'mess' },
  { k: 'cloth', label: 'Cloth simulation', type: 'bool', def: true, mirror: 'cloth' },
  { k: 'cloth_steps', label: 'Cloth substeps', type: 'int', def: 1, min: 1, max: 8 },
  { k: 'cloth_cg', label: 'Cloth CG iterations', type: 'int', def: 20, min: 5, max: 200 },
  { grp: 'Geometry & post' },
  { k: 'env.MESH_BUDGET_SCALE', label: 'Mesh detail x', type: 'range', def: 1, min: 0.25, max: 4, step: 0.25 },
  { k: 'env.MESH_BUDGET', label: 'Triangle budget', type: 'bool', def: true, offval: '0', tip: 'off = keep full Infinigen mesh density (huge)' },
  { k: 'env.NO_SNAP', label: 'Snap to visible surfaces', type: 'bool', def: true, inv: true },
  { k: 'env.NO_WALLPUSH', label: 'Push out of walls', type: 'bool', def: true, inv: true },
  { k: 'cams', label: 'Cameras', type: 'int', def: 4, min: 0, max: 12 },
];
const PRESETS = {
  fast: { steps_scale: 0.6, cloth_steps: 1, cloth_cg: 20 },
  full: { steps_scale: 1.0, cloth_steps: 2, cloth_cg: 40 },
  draft: { steps_scale: 0.25, cloth_steps: 1, cloth_cg: 10, mess: 0.3, 'env.MESH_BUDGET_SCALE': 0.5, cams: 2 },
};
function cfgDefaults() { const d = {}; for (const f of CFG) if (f.k) d[f.k] = f.def; return d; }
let cfgVals = cfgDefaults();
function cfgRender() {
  const body = $('cfgbody'); body.innerHTML = '';
  for (const f of CFG) {
    if (f.grp) { const g = document.createElement('div'); g.className = 'grp'; g.textContent = f.grp; body.appendChild(g); continue; }
    const row = document.createElement('div'); row.className = 'row'; if (f.tip) row.title = f.tip;
    const lab = document.createElement('label'); lab.textContent = f.label; row.appendChild(lab);
    let inp; const v = cfgVals[f.k];
    if (f.type === 'bool') { inp = document.createElement('input'); inp.type = 'checkbox'; inp.checked = !!v; inp.onchange = () => cfgSet(f, inp.checked); row.appendChild(inp); }
    else if (f.type === 'range') {
      inp = document.createElement('input'); inp.type = 'range'; Object.assign(inp, { min: f.min, max: f.max, step: f.step }); inp.value = v;
      const out = document.createElement('span'); out.className = 'v'; out.textContent = (+v).toFixed(2);
      inp.oninput = () => { out.textContent = (+inp.value).toFixed(2); cfgSet(f, +inp.value); }; row.appendChild(inp); row.appendChild(out);
    } else { inp = document.createElement('input'); inp.type = 'number'; Object.assign(inp, { min: f.min, max: f.max, step: 1 }); inp.value = v == null ? '' : v; inp.placeholder = 'auto'; inp.onchange = () => cfgSet(f, inp.value === '' ? null : Math.round(+inp.value)); row.appendChild(inp); }
    body.appendChild(row);
  }
  cfgShow();
}
function cfgSet(f, v, noCustom) {
  cfgVals[f.k] = v;
  if (f.mirror === 'mess') { $('mess').value = v; $('messv').textContent = (+v).toFixed(2); }
  if (f.mirror === 'cloth') $('cloth').checked = !!v;
  if (!noCustom) $('quality').value = 'custom';
  cfgShow();
}
function applyQuality(name) {
  if (!PRESETS[name]) return;
  for (const [k, v] of Object.entries(PRESETS[name])) cfgVals[k] = v;
  $('mess').value = cfgVals.mess; $('messv').textContent = (+cfgVals.mess).toFixed(2); $('cloth').checked = cfgVals.cloth;
  cfgRender();
}
function buildOpts() {
  const o = { env: {} };
  for (const f of CFG) {
    if (!f.k) continue;
    let v = cfgVals[f.k];
    if (f.k.startsWith('env.')) {
      const name = f.k.slice(4);
      if (f.type === 'bool') { if (f.inv) { if (!v) o.env[name] = true; } else if (f.offval !== undefined) { if (!v) o.env[name] = f.offval; } else if (v) o.env[name] = true; }
      else if (v != null && v !== '') o.env[name] = String(v);
    } else o[f.k] = v;
  }
  const t = $('cfgjson').value.trim();
  if (t) { try { const x = JSON.parse(t); Object.assign(o.env, x.env || {}); delete x.env; Object.assign(o, x); $('cfgjson').style.borderColor = ''; } catch (_) { $('cfgjson').style.borderColor = '#f98'; } }
  return o;
}
function cfgShow() { $('cfgout').textContent = JSON.stringify(buildOpts()); try { localStorage.setItem('indoorgen.cfg', JSON.stringify({ v: cfgVals, j: $('cfgjson').value, q: $('quality').value })); } catch (_) {} }
$('quality').onchange = () => { if ($('quality').value !== 'custom') { cfgVals = cfgDefaults(); applyQuality($('quality').value); cfgShow(); } };
$('cfgreset').onclick = () => { cfgVals = cfgDefaults(); $('cfgjson').value = ''; const q = $('quality').value === 'custom' ? 'fast' : $('quality').value; $('quality').value = q; applyQuality(q); };
$('cfgjson').oninput = cfgShow;
$('cfglink').onclick = async () => {
  const u = new URL(location.href); u.search = '';
  u.searchParams.set('seed', $('seed').value); u.searchParams.set('room', $('room').value);
  u.searchParams.set('cfg', btoa(JSON.stringify(buildOpts())));
  if (rulesLinkParam()) u.searchParams.set('rules', rulesLinkParam());
  try { await navigator.clipboard.writeText(u.toString()); $('cfglink').textContent = 'Copied'; } catch (_) { prompt('Link', u.toString()); }
  setTimeout(() => ($('cfglink').textContent = 'Copy link'), 1500);
};
$('mess').addEventListener('input', () => cfgSet(CFG.find((f) => f.k === 'mess'), +$('mess').value, true));
$('cloth').addEventListener('change', () => cfgSet(CFG.find((f) => f.k === 'cloth'), $('cloth').checked, true));
(() => {
  try { const st = JSON.parse(localStorage.getItem('indoorgen.cfg') || 'null'); if (st) { Object.assign(cfgVals, st.v); $('cfgjson').value = st.j || ''; $('quality').value = st.q || 'fast'; } } catch (_) {}
  const u = new URLSearchParams(location.search);
  if (u.has('cfg')) { try { const o = JSON.parse(atob(u.get('cfg'))); cfgVals = cfgDefaults(); $('cfgjson').value = JSON.stringify(o); $('quality').value = 'custom'; } catch (_) {} }
  if (u.has('quality') && PRESETS[u.get('quality')]) { $('quality').value = u.get('quality'); cfgVals = cfgDefaults(); applyQuality(u.get('quality')); }
  $('mess').value = cfgVals.mess; $('messv').textContent = (+cfgVals.mess).toFixed(2); $('cloth').checked = cfgVals.cloth;
  cfgRender();
})();

function generate() {
  if (!workerReady) return;
  if (rulesErrors().length) { $('rules').open = true; setStatus('fix the furniture rules first: ' + rulesErrors()[0], true); return; }
  busy(true);
  t0 = performance.now();
  tick = setInterval(() => { $('status').dataset.t = ((performance.now() - t0) / 1000).toFixed(1); if (!$('status').classList.contains('err') && /\.\.\.$/.test($('status').textContent.replace(/ \(\d+\.\ds\)$/, ''))) $('status').textContent = $('status').textContent.replace(/ \(\d+\.\ds\)$/, '') + ` (${$('status').dataset.t}s)`; }, 200);
  worker.postMessage({
    type: 'gen', id: ++jobId, seed: +$('seed').value, room: $('room').value,
    opts: { ...buildOpts(), ...(getRules() ? { rules: getRules() } : {}) },
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
let last = performance.now();
renderer.setAnimationLoop(() => {
  const now = performance.now(), dt = Math.min(0.1, (now - last) / 1000);
  last = now;
  if (mode === 'walk') walkStep(dt); else orbit.update();
  renderer.render(scene, cam);
});

window.__ig = { scene, cam, renderer, applyPreset, topDown, setCeiling, setMode, get presets() { return presets; }, generate };
loadFromQuery().catch((e) => setStatus('load failed: ' + e, true));
if (!new URLSearchParams(location.search).has('glb')) startWorker();
