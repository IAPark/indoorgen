import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

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
  if (m.type === 'ready') { workerReady = true; $('gen').disabled = false; setStatus('ready'); { const u = new URLSearchParams(location.search); if (u.has('seed')) $('seed').value = u.get('seed'); if (u.has('room')) $('room').value = u.get('room'); if (u.has('mess')) $('mess').value = u.get('mess'); if (u.has('quality')) $('quality').value = u.get('quality'); if (u.has('auto')) generate(); } }
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

function generate() {
  if (!workerReady) return;
  busy(true);
  t0 = performance.now();
  tick = setInterval(() => { $('status').dataset.t = ((performance.now() - t0) / 1000).toFixed(1); if (!$('status').classList.contains('err') && /\.\.\.$/.test($('status').textContent.replace(/ \(\d+\.\ds\)$/, ''))) $('status').textContent = $('status').textContent.replace(/ \(\d+\.\ds\)$/, '') + ` (${$('status').dataset.t}s)`; }, 200);
  worker.postMessage({
    type: 'gen', id: ++jobId, seed: +$('seed').value, room: $('room').value,
    opts: Object.assign({ mess: +$('mess').value, cloth: $('cloth').checked, cams: 4, dense: true }, $('quality').value === 'full' ? { cloth_steps: 2, cloth_cg: 40, steps_scale: 1.0 } : {}),
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
