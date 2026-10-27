// Generation worker. Messages in:  {type:'init', wasm?:ArrayBuffer}  {type:'gen', id, seed, room, opts (a GenConfig object)}
// Messages out: {type:'status', text} {type:'ready', schema, defaults (JSON strings of the GenConfig schema / web default config)} {type:'initfail', error} {type:'done', id, glb:ArrayBuffer, ms} {type:'fail', id, error}
let rulesInfoFn = null, gen = null, sceneJson = null, schemaFn = null, defaultsFn = null;

async function init(wasmBytes) {
  const mod = await import('./pkg/ig_wasm.js');
  postMessage({ type: 'status', text: 'compiling wasm...' });
  await mod.default(wasmBytes ? { module_or_path: wasmBytes } : undefined);
  gen = mod.generate;
  sceneJson = mod.last_scene_json || null;
  rulesInfoFn = mod.rules_info || null;
  schemaFn = mod.config_schema || null; defaultsFn = mod.default_config || null;
  if (mod.set_progress) mod.set_progress((t) => postMessage({ type: 'status', text: String(t) }));
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    if (m.type === 'init') {
      await init(m.wasm);
      if (rulesInfoFn) { try { postMessage({ type: 'rulesinfo', info: rulesInfoFn() }); } catch (e) { postMessage({ type: 'status', text: 'rules info: ' + e }); } }
      postMessage({ type: 'ready', schema: schemaFn ? schemaFn() : null, defaults: defaultsFn ? defaultsFn() : null });
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
