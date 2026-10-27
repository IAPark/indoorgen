// Furniture rules editor (section #rules). Model = the RuleSpec of src/rules.rs:
//   { rooms: { Bedroom: { replace: false, rules: [{class, relation, min, max, prefer, weight}] } }, disable: [names] }
// API: rulesInit(info) with the JSON of wasm rules_info(); getRules() -> spec | null (null = nothing set) ; rulesErrors() -> [string]
const $ = (id) => document.getElementById(id);
let info = null, spec = { rooms: {}, disable: [] }, tab = 'Bedroom', errors = [];
let IG = null, mount = null;
const REL_KINDS = ['on_floor', 'against_wall', 'on', 'ontop', 'beside'];

const el = (tag, props = {}, ...kids) => { const e = Object.assign(document.createElement(tag), props); for (const k of kids) e.append(k); return e; };
const relKind = (r) => (r || 'on_floor').split(':')[0];
const relParent = (r) => (r || '').includes(':') ? r.split(':')[1] : '';

function save() { if (IG) IG.set('rules', getRules(), true); }
function room(t) { return (spec.rooms[t] ||= { replace: false, rules: [] }); }

export function rulesErrors() { return errors; }
/** the spec to send to generate(); null when empty */
export function getRules() {
  validate();
  const out = { rooms: {}, disable: spec.disable.filter(Boolean) };
  for (const [t, rr] of Object.entries(spec.rooms)) {
    const rules = rr.rules.filter((r) => r.class).map((r) => {
      const o = { class: r.class, relation: r.relation || 'on_floor' };
      if (r.min != null && r.min !== '') o.min = +r.min;
      if (r.max != null && r.max !== '') o.max = +r.max;
      if (r.prefer) { o.prefer = r.prefer; if (r.weight != null && r.weight !== '') o.weight = +r.weight; }
      return o;
    });
    if (rules.length || rr.replace) out.rooms[t] = { replace: !!rr.replace, rules };
  }
  return Object.keys(out.rooms).length || out.disable.length ? out : null;
}

function known(c) { return info && (info.classes.factories.includes(c) || info.classes.tags.includes(c) || c === 'Plant'); }
function validate() {
  errors = [];
  if (!info) return;
  for (const [t, rr] of Object.entries(spec.rooms)) rr.rules.forEach((r, i) => {
    const at = `${t} rule ${i + 1}`;
    r._err = '';
    const bad = (m) => { r._err = m; errors.push(`${at}: ${m}`); };
    if (!r.class) { if (r.min != null && r.min !== '' || r.max != null && r.max !== '') bad('class missing'); return; }
    if (!known(r.class)) bad(`unknown class '${r.class}'`);
    else if (['on', 'ontop', 'beside'].includes(relKind(r.relation)) && !known(relParent(r.relation))) bad(`unknown parent class '${relParent(r.relation)}'`);
    else if (r.min !== '' && r.min != null && r.max !== '' && r.max != null && +r.min > +r.max) bad('min > max');
    else if ((r.min == null || r.min === '') && (r.max == null || r.max === '') && !r.prefer) bad('set min, max or prefer');
  });
  const names = new Set(info.constraint_names);
  for (const d of spec.disable) if (d && !names.has(d)) errors.push(`disable: unknown constraint '${d}'`);
}

function render() {
  const root = mount; if (!root || !info) return;
  validate();
  root.textContent = '';
  const tabs = el('div', { className: 'rtabs' });
  for (const t of info.rooms) {
    const n = (spec.rooms[t]?.rules || []).filter((r) => r.class).length;
    tabs.append(el('button', { className: t === tab ? 'on' : '', textContent: (t === 'Any' ? 'All rooms' : t.replace(/([a-z])([A-Z])/g, '$1 $2')) + (n ? ` (${n})` : ''), onclick: () => { tab = t; render(); } }));
  }
  root.append(tabs);
  const rr = spec.rooms[tab] || { replace: false, rules: [] };
  if (tab !== 'Any') {
    const rep = el('input', { type: 'checkbox', checked: !!rr.replace, onchange: () => { room(tab).replace = rep.checked; save(); render(); } });
    root.append(el('label', { className: 'rrep', title: 'Drop ALL built-in rules of this room type (you then define everything; e.g. no sink unless you add one)' }, rep, ' replace built-in rules'));
  }
  const dl = el('datalist', { id: 'rclasses' });
  for (const c of [...info.classes.tags, 'Plant', ...info.classes.factories]) dl.append(el('option', { value: c }));
  root.append(dl);
  const tbl = el('table', { className: 'rtbl' });
  tbl.append(el('tr', {}, ...['class', 'where', 'min', 'max', 'prefer', ''].map((h) => el('th', { textContent: h }))));
  rr.rules.forEach((r, i) => {
    const upd = (k, v) => { r[k] = v; save(); validate(); show(); };
    const cls = el('input', { value: r.class || '', list: 'rclasses', placeholder: 'SofaFactory', oninput: () => { upd('class', cls.value.trim()); } });
    const kind = el('select', { onchange: () => { r.relation = kind.value + (['on', 'ontop', 'beside'].includes(kind.value) ? ':' + (par.value || 'Storage') : ''); save(); render(); } });
    for (const k of REL_KINDS) kind.append(el('option', { value: k, textContent: { on_floor: 'on floor', against_wall: 'against wall', on: 'on (shelf)', ontop: 'on top of', beside: 'beside' }[k], selected: k === relKind(r.relation) }));
    const par = el('input', { className: 'par', value: relParent(r.relation), list: 'rclasses', placeholder: 'class', oninput: () => { r.relation = kind.value + ':' + par.value.trim(); save(); validate(); show(); } });
    par.style.display = ['on', 'ontop', 'beside'].includes(relKind(r.relation)) ? '' : 'none';
    const num = (k) => el('input', { type: 'number', min: 0, step: 1, value: r[k] ?? '', oninput: (e) => upd(k, e.target.value === '' ? null : Math.max(0, Math.round(+e.target.value))) });
    const pref = el('select', { onchange: () => { r.prefer = pref.value || null; save(); render(); } });
    for (const [v, t] of [['', '-'], ['more', 'more'], ['fewer', 'fewer']]) pref.append(el('option', { value: v, textContent: t, selected: v === (r.prefer || '') }));
    const wt = el('input', { type: 'number', className: 'wt', min: 0, step: 0.5, value: r.weight ?? '', placeholder: '1', title: 'preference weight', oninput: (e) => upd('weight', e.target.value === '' ? null : +e.target.value) });
    wt.style.display = r.prefer ? '' : 'none';
    const del = el('button', { textContent: '×', title: 'remove', onclick: () => { rr.rules.splice(i, 1); save(); render(); } });
    const tr = el('tr', { className: r._err ? 'bad' : '', title: r._err || '' });
    tr.append(el('td', {}, cls), el('td', {}, kind, par), el('td', {}, num('min')), el('td', {}, num('max')), el('td', {}, pref, wt), el('td', {}, del));
    tbl.append(tr);
  });
  root.append(tbl);
  const btns = el('div', { className: 'btns' },
    el('button', { id: 'radd', textContent: '+ Add rule', onclick: () => { room(tab).rules.push({ class: '', relation: 'on_floor', min: 1, max: null }); save(); render(); } }),
    el('button', { textContent: 'Load built-in', title: 'Fill this tab with the built-in rules (class / where / min / max) to edit them', onclick: () => {
      const d = info.defaults.rooms[tab]; if (!d) return;
      const cur = room(tab);
      for (const r of d.rules) { const ex = cur.rules.find((x) => x.class === r.class && x.relation === r.relation); if (ex) { ex.min = r.min ?? null; ex.max = r.max ?? null; } else cur.rules.push({ ...r }); }
      save(); render();
    } }),
    el('button', { textContent: 'Clear tab', onclick: () => { delete spec.rooms[tab]; save(); render(); } }),
  );
  root.append(btns);
  const dis = el('input', { className: 'dis', value: spec.disable.join(', '), list: 'rnames', placeholder: 'e.g. sofa_positioning, tv', oninput: () => { spec.disable = dis.value.split(/[\s,]+/).filter(Boolean); save(); validate(); show(); } });
  const nl = el('datalist', { id: 'rnames' }); for (const n of info.constraint_names) nl.append(el('option', { value: n }));
  root.append(el('label', { className: 'sub', textContent: 'Disable built-in constraints (names, comma separated)' }), dis, nl);
  const ta = el('textarea', { id: 'rulesjson', rows: 4, spellcheck: false, placeholder: 'rules JSON (paste to import)', value: JSON.stringify(getRules() || {}, null, 1), onchange: () => {
    try { const s = JSON.parse(ta.value || '{}'); spec = { rooms: s.rooms || {}, disable: s.disable || [] }; save(); render(); } catch (e) { ta.style.borderColor = '#f98'; $('ruleserr').textContent = 'JSON: ' + e.message; }
  } });
  root.append(el('label', { className: 'sub', textContent: 'Rules JSON (same format as gen2 --rules)' }), ta, el('div', { id: 'ruleserr', className: 'rerr' }));
  show();
}
function show() {
  const e = $('ruleserr'); if (e) e.textContent = errors.join('\n');
  const s = document.querySelector('#rules-sec > summary'); if (s) { const g = getRules(); const n = g ? Object.values(g.rooms).reduce((a, r) => a + r.rules.length, 0) : 0; s.textContent = `Rules${n ? ` (${n} custom)` : ''}${errors.length ? ' - errors' : ''}`; }
  const ta = $('rulesjson'); if (ta && document.activeElement !== ta) { ta.value = JSON.stringify(getRules() || {}, null, 1); ta.style.borderColor = ''; }
}

export function rulesInit(infoJson, ig) {
  info = typeof infoJson === 'string' ? JSON.parse(infoJson) : infoJson;
  IG = ig; mount = ig.rulesMount;
  const load = (r) => { spec = { rooms: structuredClone(r?.rooms || {}), disable: [...(r?.disable || [])] }; };
  load(IG.get().rules);
  if (Object.keys(spec.rooms).length || spec.disable.length) { const d = document.getElementById('cfg'); if (d) d.open = true; document.getElementById('rules-sec').open = true; }
  IG.onChange((cfg) => { if (JSON.stringify(cfg.rules ?? null) !== JSON.stringify(getRules())) { load(cfg.rules); render(); } });
  render();
}
