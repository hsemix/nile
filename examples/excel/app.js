import { COLS, ROWS, address, position, calculate } from './formulas.js';
const el = (id) => document.getElementById(id);
const report = (text) => { el('status').textContent = text; console.log('spreadsheet:', text); };
const storageKey = 'nile-spreadsheet-v1';
function seed(factor = 1) {
  const cells = { A1: 'NORTHSTAR STUDIO', A2: 'Quarterly operating budget', A11: 'TOTAL' }, styles = {};
  ['Category', 'January', 'February', 'March', 'Q1 total', 'Monthly avg', 'Budget', 'Variance'].forEach((s, i) => { cells[address(3, i)] = s; });
  const rows = [['Design & research', 2800, 3200, 3100, 9500], ['Engineering', 5600, 6100, 5800, 18000], ['Marketing', 1400, 1650, 1500, 4800], ['Software & tools', 480, 520, 510, 1600], ['Workspace', 1200, 1200, 1200, 3600], ['Travel & events', 650, 900, 800, 2600]];
  rows.forEach((values, index) => {
    const row = index + 5; cells['A' + row] = values[0];
    ['B', 'C', 'D'].forEach((c, i) => { cells[c + row] = String(Math.round(values[i + 1] * factor)); });
    cells['E' + row] = '=SUM(B' + row + ':D' + row + ')';
    cells['F' + row] = '=AVERAGE(B' + row + ':D' + row + ')';
    cells['G' + row] = String(Math.round(values[4] * factor));
    cells['H' + row] = '=G' + row + '-E' + row;
  });
  for (const col of ['B', 'C', 'D', 'E', 'G', 'H']) cells[col + '11'] = '=SUM(' + col + '5:' + col + '10)';
  cells.F11 = '=AVERAGE(B11:D11)';
  for (let r = 4; r <= 10; r++) for (let c = 1; c < COLS; c++) styles[address(r, c)] = { format: 'currency' };
  cells.A14 = 'Tip: edit a monthly amount'; cells.A15 = 'Totals recalculate automatically.';
  cells.A17 = 'Try =SUM(B5:D5)'; cells.A18 = 'Or =(B5+C5)*2';
  return { cells, styles };
}
export let workbook = { Budget: seed(), Forecast: seed(1.12) };
export let sheetName = 'Budget', selected = 'E11';
let values = {}, editor = null, editingRef = null, tab = 'Home', formulaDirty = false, lastClick = { ref: '', time: 0 };
const histories = { Budget: [], Forecast: [] }, futures = { Budget: [], Forecast: [] };
const sheet = () => workbook[sheetName];
function validate(data) {
  for (const name of ['Budget', 'Forecast']) {
    const s = data?.[name]; if (!s || typeof s.cells !== 'object' || !s.styles || typeof s.styles !== 'object') return false;
    for (const [ref, text] of Object.entries(s.cells)) { position(ref); if (typeof text !== 'string' || text.length > 4096) return false; }
    for (const [ref, style] of Object.entries(s.styles)) { position(ref); if (!style || typeof style !== 'object' || (style.format && !['general', 'currency', 'percent'].includes(style.format)) || (style.align && !['left', 'center', 'right'].includes(style.align))) return false; }
  }
  return true;
}
try { const stored = localStorage.getItem(storageKey); if (stored) { const data = JSON.parse(stored); if (validate(data)) { workbook = data; el('save-state').textContent = 'Saved on this device'; } } } catch (error) { report('Saved workbook could not be loaded; showing sample'); }
function checkpoint() { histories[sheetName].push(JSON.stringify(sheet())); if (histories[sheetName].length > 100) histories[sheetName].shift(); futures[sheetName] = []; }
function changed() { el('save-state').textContent = 'Unsaved changes'; render(); }
function display(ref) {
  const v = values[ref], format = sheet().styles[ref]?.format;
  if (typeof v !== 'number') return String(v ?? '');
  if (format === 'currency') return (v < 0 ? '−' : '') + '$' + Math.abs(v).toFixed(0).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  if (format === 'percent') return (v * 100).toFixed(1) + '%';
  return String(Math.round(v * 10000) / 10000);
}
function buildGrid() {
  let html = '<div class="corner"></div>';
  for (let c = 0; c < COLS; c++) html += '<div class="column-head" id="column-' + c + '">' + String.fromCharCode(65 + c) + '</div>';
  for (let r = 0; r < ROWS; r++) {
    html += '<div class="row-head" id="row-' + r + '">' + (r + 1) + '</div>';
    for (let c = 0; c < COLS; c++) { const ref = address(r, c); html += '<div class="cell" id="cell-' + ref + '" data-cell="' + ref + '"><span id="value-' + ref + '"></span></div>'; }
  }
  el('grid').innerHTML = html;
}
function ribbon() {
  const b = (label, action, extra = '') => '<button data-action="' + action + '" ' + extra + '>' + label + '</button>';
  const group = (content, label) => '<div class="group"><div class="tools">' + content + '</div><div class="group-label">' + label + '</div></div>';
  if (tab === 'Home') el('ribbon').innerHTML = group('<div class="stack">' + b('Undo', 'undo') + b('Redo', 'redo') + '</div>', 'History') + group('<div class="stack"><div class="row"><select id="number-format"><option value="general">General</option><option value="currency">Currency</option><option value="percent">Percentage</option></select></div><div class="row">' + b('<b>B</b>', 'bold', 'class="square"') + b('Fill', 'fill') + b('Clear', 'clear-format') + '</div></div>', 'Cell formatting') + group('<div class="stack">' + b('Align left', 'align-left') + b('Align right', 'align-right') + '</div>', 'Alignment') + group('<button class="large" data-action="sum"><span class="large-icon">Σ</span><span>AutoSum</span></button><button class="large" data-action="average"><span class="large-icon">x̄</span><span>Average</span></button>', 'Formulas') + group('<div class="stack">' + b('Clear cell', 'clear') + b('Edit cell', 'edit') + '</div>', 'Editing') + group('<div class="stack"><span style="color:#7e9586;padding:6px;font-size:11px">Live formulas · two worksheets</span><span style="color:#8ca093;padding:0 6px;font-size:10px">Enter values or =SUM(B5:D5)</span></div>', 'Workbook');
  else if (tab === 'File') el('ribbon').innerHTML = group('<div class="stack">' + b('Save workbook', 'save') + b('Reset this sheet', 'reset') + '</div>', 'Local workbook') + group('<div class="stack"><span style="padding:10px;color:#7c9283">Save stores both sheets in localStorage.</span><span style="padding:0 10px;color:#7c9283">No .xlsx import or export in this example.</span></div>', 'About');
  else if (tab === 'Formulas') el('ribbon').innerHTML = group(b('Σ AutoSum', 'sum') + b('Average', 'average'), 'Insert formula') + group('<div class="stack"><span style="padding:8px;color:#6d8878">SUM · AVERAGE · MIN · MAX · COUNT</span><span style="padding:0 8px;color:#81998b">References A1:H24 · arithmetic + − × ÷ · parentheses</span></div>', 'Supported functions');
  else el('ribbon').innerHTML = group('<div class="stack">' + b('Recalculate', 'recalculate') + b('Reset this sheet', 'reset') + '</div>', 'Data') + group('<span style="padding:10px;color:#7e9586">Change a value to recalculate all dependent formulas.</span>', 'Calculation');
}
export function render() {
  values = calculate(sheet().cells);
  for (let r = 0; r < ROWS; r++) for (let c = 0; c < COLS; c++) {
    const ref = address(r, c), cell = el('cell-' + ref), style = sheet().styles[ref] || {}, value = values[ref];
    cell.className = 'cell' + (typeof value === 'number' ? ' number' : '') + (r === 3 ? ' heading' : '') + (r === 10 ? ' total' : '') + (r === 0 ? ' title' : '') + (r === 1 ? ' subtitle' : '') + (ref === selected ? ' selected' : '') + (typeof value === 'string' && value.startsWith('#') ? ' error' : '');
    cell.style.fontWeight = style.bold ? '700' : ''; cell.style.backgroundColor = style.fill ? '#fff0bc' : ''; cell.style.textAlign = style.align || '';
    el('value-' + ref).textContent = display(ref);
  }
  const [r, c] = position(selected);
  document.querySelectorAll('.column-head').forEach((h, i) => h.classList.toggle('active', i === c));
  document.querySelectorAll('.row-head').forEach((h, i) => h.classList.toggle('active', i === r));
  el('name-box').value = selected; el('formula').value = sheet().cells[selected] || '';
  formulaDirty = false;
  const format = el('number-format'); if (format) format.value = sheet().styles[selected]?.format || 'general';
  document.querySelectorAll('[data-action="bold"]').forEach((b) => b.classList.toggle('on', !!sheet().styles[selected]?.bold));
  document.querySelectorAll('[data-action="fill"]').forEach((b) => b.classList.toggle('on', !!sheet().styles[selected]?.fill));
  document.querySelectorAll('[data-action="undo"]').forEach((b) => { b.disabled = !histories[sheetName].length; });
  document.querySelectorAll('[data-action="redo"]').forEach((b) => { b.disabled = !futures[sheetName].length; });
  el('cell-info').textContent = sheetName + ' / ' + selected + (String(sheet().cells[selected] || '').startsWith('=') ? ' · Formula result: ' + display(selected) : ' · ' + (typeof values[selected] === 'number' ? 'Number' : 'Text'));
}
export function setCell(ref, raw) { position(ref); raw = String(raw).slice(0, 4096); if ((sheet().cells[ref] || '') === raw) return; checkpoint(); if (raw) sheet().cells[ref] = raw; else delete sheet().cells[ref]; changed(); report(ref + ' updated'); }
export function select(ref, focus = true) { position(ref); finishEdit(); commitFormula(); selected = ref; render(); if (focus) el('viewport').focus(); reveal(); }
function commitFormula() { if (formulaDirty) { const value = el('formula').value; formulaDirty = false; setCell(selected, value); } }
function reveal() {
  const viewport = el('viewport'), a = viewport.getBoundingClientRect(), b = el('cell-' + selected).getBoundingClientRect();
  if (b.bottom > a.bottom) viewport.scrollTop += b.bottom - a.bottom; else if (b.top < a.top) viewport.scrollTop -= a.top - b.top;
  if (b.right > a.right) viewport.scrollLeft += b.right - a.right; else if (b.left < a.left) viewport.scrollLeft -= a.left - b.left;
}
function move(dr, dc) { const [r, c] = position(selected); select(address(Math.max(0, Math.min(ROWS - 1, r + dr)), Math.max(0, Math.min(COLS - 1, c + dc)))); }
export function beginEdit(initial) {
  if (editor) return; const input = document.createElement('input'); input.id = 'cell-editor';
  input.value = initial === undefined ? sheet().cells[selected] || '' : initial;
  editingRef = selected; editor = input; el('cell-' + selected).appendChild(input);
  input.addEventListener('input', () => { el('formula').value = input.value; });
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') { finishEdit(false); el('viewport').focus(); event.preventDefault(); }
    if (event.key === 'Enter' || event.key === 'Tab') { finishEdit(); if (event.key === 'Enter') move(event.shiftKey ? -1 : 1, 0); else move(0, event.shiftKey ? -1 : 1); event.preventDefault(); }
  });
  input.addEventListener('blur', () => finishEdit()); input.focus();
  if (initial === undefined) input.select(); else { const n = Array.from(input.value).length; input.setSelectionRange(n, n); }
}
function finishEdit(commit = true) {
  if (!editor) return; const input = editor, ref = editingRef, value = input.value; editor = null; editingRef = null;
  input.remove(); if (commit) setCell(ref, value); else render();
}
function undoRedo(redo) {
  finishEdit(); const from = redo ? futures[sheetName] : histories[sheetName], to = redo ? histories[sheetName] : futures[sheetName];
  if (!from.length) return; to.push(JSON.stringify(sheet())); workbook[sheetName] = JSON.parse(from.pop()); changed(); report(redo ? 'Redo' : 'Undo');
}
function action(name) {
  if (name === 'cancel') { finishEdit(false); render(); el('viewport').focus(); return; }
  if (name === 'commit') { if (editor) finishEdit(); else setCell(selected, el('formula').value); el('viewport').focus(); return; }
  finishEdit(); commitFormula();
  if (name === 'edit') beginEdit();
  else if (name === 'undo' || name === 'redo') undoRedo(name === 'redo');
  else if (name === 'clear') setCell(selected, '');
  else if (name === 'save') { try { localStorage.setItem(storageKey, JSON.stringify(workbook)); el('save-state').textContent = 'Saved on this device'; report('Workbook saved locally'); } catch (error) { report('Save failed: ' + error.message); } }
  else if (name === 'reset') { checkpoint(); workbook[sheetName] = seed(sheetName === 'Forecast' ? 1.12 : 1); changed(); report('Sample sheet restored · Undo to recover edits'); }
  else if (name === 'recalculate') { render(); report('All formulas recalculated'); }
  else if (name === 'sum' || name === 'average') { const [r, c] = position(selected); if (!r) { report('Choose a cell below the values to aggregate'); return; } let start = r - 1; while (start > 0 && typeof values[address(start - 1, c)] === 'number') start--; setCell(selected, '=' + (name === 'sum' ? 'SUM' : 'AVERAGE') + '(' + address(start, c) + ':' + address(r - 1, c) + ')'); }
  else { checkpoint(); const style = sheet().styles[selected] || (sheet().styles[selected] = {}); if (name === 'bold' || name === 'fill') style[name] = !style[name]; else if (name === 'clear-format') delete sheet().styles[selected]; else if (name.startsWith('align-')) style.align = name.slice(6); changed(); }
}
document.addEventListener('mousedown', (event) => { if (event.target.closest('[data-action]')) event.preventDefault(); });
document.addEventListener('click', (event) => {
  const b = event.target.closest('[data-action]'); if (b && !b.disabled) { action(b.dataset.action); return; }
  const cell = event.target.closest('[data-cell]'); if (cell && event.target !== editor) { const ref = cell.dataset.cell, now = Date.now(); if (ref === selected && lastClick.ref === ref && now - lastClick.time < 450) beginEdit(); else select(ref); lastClick = { ref, time: now }; return; }
  const t = event.target.closest('[data-tab]'); if (t) { finishEdit(); commitFormula(); tab = t.dataset.tab; document.querySelectorAll('[data-tab]').forEach((b) => b.classList.toggle('active', b === t)); ribbon(); render(); }
  const s = event.target.closest('[data-sheet]'); if (s) { finishEdit(); commitFormula(); sheetName = s.dataset.sheet; selected = 'E11'; document.querySelectorAll('[data-sheet]').forEach((b) => b.classList.toggle('active', b === s)); render(); report(sheetName + ' sheet'); }
});
el('formula').addEventListener('input', () => { formulaDirty = true; });
el('formula').addEventListener('keydown', (event) => {
  if (event.key === 'Enter') { setCell(selected, event.target.value); move(1, 0); event.preventDefault(); }
  if (event.key === 'Escape') { render(); el('viewport').focus(); event.preventDefault(); }
});
el('formula').addEventListener('change', (event) => { setCell(selected, event.target.value); });
el('name-box').addEventListener('keydown', (event) => { if (event.key === 'Enter') { try { select(event.target.value.trim().toUpperCase()); } catch (error) { render(); report('Use a cell address from A1 to H24'); } event.preventDefault(); } });
document.addEventListener('change', (event) => { if (event.target.id === 'number-format') { checkpoint(); const style = sheet().styles[selected] || (sheet().styles[selected] = {}); style.format = event.target.value; changed(); } });
el('viewport').addEventListener('keydown', (event) => {
  if (event.target !== el('viewport')) return;
  const key = event.key;
  if (event.ctrlKey || event.metaKey) { if (key.toLowerCase() === 'z' || key.toLowerCase() === 'y') { undoRedo(key.toLowerCase() === 'y' || event.shiftKey); event.preventDefault(); } return; }
  if (key === 'ArrowDown') move(1, 0); else if (key === 'ArrowUp') move(-1, 0); else if (key === 'ArrowLeft') move(0, -1); else if (key === 'ArrowRight') move(0, 1);
  else if (key === 'Tab') move(0, event.shiftKey ? -1 : 1); else if (key === 'Enter' || key === 'F2') beginEdit();
  else if (key === 'Backspace' || key === 'Delete') setCell(selected, '');
  else if (Array.from(key).length === 1 && !event.altKey) beginEdit(key); else return;
  event.preventDefault();
});
document.addEventListener('keydown', (event) => { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') { action('save'); event.preventDefault(); } });
buildGrid(); ribbon(); render();
console.log('spreadsheet ready: editable cells, formulas, Budget and Forecast');
