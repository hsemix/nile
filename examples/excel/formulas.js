// A deliberately small formula language. No eval or JavaScript execution.
export const COLS = 8, ROWS = 24;
export const address = (row, col) => String.fromCharCode(65 + col) + (row + 1);
export function position(ref) {
  const m = /^([A-H])(\d{1,2})$/.exec(ref.toUpperCase());
  if (!m || +m[2] < 1 || +m[2] > ROWS) throw new Error('#REF!');
  return [+m[2] - 1, m[1].charCodeAt(0) - 65];
}
export function calculate(cells) {
  const cache = {}, visiting = new Set();
  const fail = (message) => { throw new Error(message); };
  function numeric(value) {
    if (typeof value === 'string' && value.startsWith('#')) fail(value);
    if (value === '' || value == null) return 0;
    const n = Number(value); if (!Number.isFinite(n)) fail('#VALUE!'); return n;
  }
  function evaluate(ref) {
    position(ref);
    if (Object.prototype.hasOwnProperty.call(cache, ref)) return cache[ref];
    if (visiting.has(ref)) return '#CYCLE!';
    visiting.add(ref);
    const raw = String(cells[ref] ?? '');
    try {
      cache[ref] = raw[0] === '=' ? parse(raw.slice(1)) : raw.trim() !== '' && Number.isFinite(Number(raw)) ? Number(raw) : raw;
    } catch (error) { cache[ref] = error.message.startsWith('#') ? error.message : '#ERROR!'; }
    visiting.delete(ref); return cache[ref];
  }
  function parse(source) {
    if (source.length > 512) fail('#ERROR!');
    const tokens = source.toUpperCase().match(/(?:\d+(?:\.\d*)?|\.\d+)|[A-Z]+\d*|[+\-*/():,]|\S/g) || [];
    let at = 0;
    const take = (token) => { if (tokens[at] !== token) fail('#ERROR!'); at++; };
    function range() {
      const start = tokens[at++]; position(start || ''); take(':');
      const end = tokens[at++], [r1, c1] = position(start), [r2, c2] = position(end || '');
      const values = [];
      for (let r = Math.min(r1, r2); r <= Math.max(r1, r2); r++) for (let c = Math.min(c1, c2); c <= Math.max(c1, c2); c++) {
        const value = evaluate(address(r, c));
        if (typeof value === 'string' && value.startsWith('#')) fail(value);
        // Aggregate ranges ignore blank and text cells.
        if (typeof value === 'number') values.push(value);
      }
      return values;
    }
    function primary() {
      const t = tokens[at];
      if (t === '+' || t === '-') { at++; return (t === '-' ? -1 : 1) * primary(); }
      if (t === '(') { at++; const n = expression(); take(')'); return n; }
      if (/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(t || '')) { at++; return Number(t); }
      if (/^[A-Z]+\d+$/.test(t || '')) { at++; return numeric(evaluate(t)); }
      if (['SUM', 'AVERAGE', 'MIN', 'MAX', 'COUNT'].includes(t)) {
        at++; take('('); const values = [];
        if (tokens[at] !== ')') do {
          if (tokens[at + 1] === ':') values.push(...range()); else values.push(expression());
          if (tokens[at] !== ',') break; at++;
        } while (true);
        take(')');
        if (t === 'COUNT') return values.length;
        if (t === 'SUM') return values.reduce((a, b) => a + b, 0);
        if (!values.length) return t === 'AVERAGE' ? fail('#DIV/0!') : 0;
        if (t === 'AVERAGE') return values.reduce((a, b) => a + b, 0) / values.length;
        return t === 'MIN' ? Math.min(...values) : Math.max(...values);
      }
      fail('#ERROR!');
    }
    function product() {
      let n = primary();
      while (tokens[at] === '*' || tokens[at] === '/') { const op = tokens[at++], b = primary(); if (op === '/' && b === 0) fail('#DIV/0!'); n = op === '*' ? n * b : n / b; }
      return n;
    }
    function expression() {
      let n = product();
      while (tokens[at] === '+' || tokens[at] === '-') { const op = tokens[at++], b = product(); n = op === '+' ? n + b : n - b; }
      return n;
    }
    const result = expression(); if (at !== tokens.length) fail('#ERROR!');
    if (!Number.isFinite(result)) fail('#VALUE!'); return result;
  }
  for (let r = 0; r < ROWS; r++) for (let c = 0; c < COLS; c++) evaluate(address(r, c));
  return cache;
}
