// Generation worker. Messages in:  {type:'init', wasm?:ArrayBuffer}  {type:'gen', id, seed, room, opts}
// Messages out: {type:'status', text} {type:'ready'} {type:'initfail', error} {type:'done', id, glb:ArrayBuffer, ms} {type:'fail', id, error}
let gen = null, sceneJson = null;

async function init(wasmBytes) {
  const mod = await import('./pkg/ig_wasm.js');
  postMessage({ type: 'status', text: 'compiling wasm...' });
  await mod.default(wasmBytes ? { module_or_path: wasmBytes } : undefined);
  gen = mod.generate;
  sceneJson = mod.last_scene_json || null;
  if (mod.rules_info) { try { postMessage({ type: 'rulesinfo', info: mod.rules_info() }); } catch (e) { postMessage({ type: 'status', text: 'rules info: ' + e }); } }
  if (mod.set_progress) mod.set_progress((t) => postMessage({ type: 'status', text: String(t) }));
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    if (m.type === 'init') {
      await init(m.wasm);
      postMessage({ type: 'ready' });
    } else if (m.type === 'gen') {
      const t0 = performance.now();
      postMessage({ type: 'status', text: `generating ${m.room} seed ${m.seed}...` });
      const out = gen(m.seed >>> 0, m.room, JSON.stringify(m.opts));
      const buf = out.buffer.byteLength === out.byteLength ? out.buffer : out.slice().buffer;
      let scene = null;
      try { scene = sceneJson ? sceneJson() : null; } catch (_) {}
      postMessage({ type: 'done', id: m.id, glb: buf, scene, ms: performance.now() - t0 }, [buf]);
    }
  } catch (err) {
    postMessage({ type: m.type === 'init' ? 'initfail' : 'fail', id: m.id, error: String(err && err.message || err) });
  }
};
