// node tools/wasm/node_run.cjs SEED ROOM [OPTS_JSON] [OUT.glb]   (needs WASM_TARGET=nodejs tools/wasm/build.sh)
const fs = require("fs");
const path = require("path");
const m = require(path.join(__dirname, "../../web/pkg-node/ig_wasm.js"));
const seed = parseInt(process.argv[2] || "5");
const room = process.argv[3] || "Bedroom";
const opts = process.argv[4] || '{"mess":0.5,"cloth":true,"cams":4,"dense":true}';
const out = process.argv[5] || "/tmp/ig_wasm.glb";
const t0 = performance.now();
let last = t0;
m.set_progress((msg) => {
  const t = performance.now();
  console.error(`[${((t - t0) / 1000).toFixed(1)}s +${((t - last) / 1000).toFixed(1)}s] ${msg}`);
  last = t;
});
const glb = m.generate(seed, room, opts);
const t1 = performance.now();
fs.writeFileSync(out, glb);
const side = m.last_scene_json();
fs.writeFileSync(out.replace(/\.glb$/, "") + ".json", side);
const mem = m.__wasm ? m.__wasm.memory.buffer.byteLength : 0;
console.error(`generate(${seed}, ${room}) ${((t1 - t0) / 1000).toFixed(1)} s; glb ${glb.length} bytes; wasm memory ${(mem / 1048576).toFixed(0)} MiB; rss ${(process.memoryUsage().rss / 1048576).toFixed(0)} MiB`);
