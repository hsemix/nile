// @nile/ui/grid: <nile-grid>, a data grid for lists of records: columns, sorting, selection, keyboard
// navigation and column resizing, drawing only the rows that are visible so 100,000 rows cost the same as 30.
//
//   <nile-grid id="receipt" key="id" empty="No items" empty-hint="Scan a product">
//     <nile-column key="name" title="Product name"></nile-column>
//     <nile-column key="qty" title="Quantity" width="90" type="number"></nile-column>
//     <nile-column key="price" title="Price" width="90" type="number"></nile-column>
//   </nile-grid>
//   receipt.rows = [{ id: 1, name: "Espresso", qty: 2, price: 3.5 }];
//
// Columns can also be set from script: `grid.columns = [{ key, title, width, minWidth, align, type, sortable,
// resizable, value(row), format(value, row), render(row, value, index) -> string | Node | { html }, compare(a, b) }]`.
// A column without a width shares the leftover space; `type: "number"` right-aligns and sorts numerically.
//
// Attributes: row-height (px, default 32), select (none | single | multiple; default single), key (the field
// that identifies a row, so the selection survives replacing `rows`), sort ("name" or "name:desc"), striped,
// header ("false" hides it), empty / empty-hint (shown when there are no rows).
// Properties: rows, columns, selected (rows, in grid order), selectedIndex, selectedCount, rowCount, mountedCount,
// sortKey, sortDirection.
// Methods: add(row), remove(rowOrIndex), update(index, patch), clear(), select(rowOrIndex...), sortBy(key, dir),
// scrollToRow(index), refresh() (after changing rows in place).
// Events: `selectionchange` (detail: { selected, indexes }), `rowactivate` (Enter or double-click; detail: { row, index }),
// `sort` (detail: { key, direction }), `columnresize` (detail: { key, width }).
// Keys: arrows, Page Up/Down, Home/End move; Shift extends (select=multiple); Space toggles, Ctrl/Cmd+A selects all.

import { style, emit, numberAttr, flagAttr } from "@nile/ui/base";

const CSS = `
:where(nile-grid) { display: flex; flex-direction: column; height: 300px; min-width: 0; min-height: 0; box-sizing: border-box; overflow: hidden;
  border: 1px solid var(--nile-grid-border, rgba(128, 128, 128, 0.35)); background: var(--nile-grid-bg, Canvas); color: var(--nile-grid-color, CanvasText); }
:where(nile-grid) > .nile-grid-head { flex: none; overflow: hidden; background: var(--nile-grid-head-bg, rgba(128, 128, 128, 0.14)); border-bottom: 1px solid var(--nile-grid-border, rgba(128, 128, 128, 0.35)); }
:where(nile-grid[header="false"]) > .nile-grid-head { display: none; }
:where(nile-grid) .nile-grid-hrow { display: grid; height: var(--nile-grid-head-height, 34px); }
:where(nile-grid) .nile-grid-hcell { position: relative; display: flex; align-items: center; gap: 4px; padding: 0 10px; box-sizing: border-box; overflow: hidden; white-space: nowrap; font-weight: 600; user-select: none; }
:where(nile-grid) .nile-grid-hcell > .nile-grid-title { overflow: hidden; text-overflow: ellipsis; }
:where(nile-grid) .nile-grid-hcell > .nile-grid-sort { flex: none; opacity: 0.7; }
:where(nile-grid) .nile-grid-resizer { position: absolute; top: 0; bottom: 0; right: 0; width: 6px; cursor: col-resize; }
:where(nile-grid) > .nile-grid-body { flex: 1; min-height: 0; overflow: auto; position: relative; outline: none; }
:where(nile-grid) .nile-grid-spacer { position: relative; }
:where(nile-grid) .nile-grid-row { position: absolute; left: 0; right: 0; display: grid; align-items: center; box-sizing: border-box; border-bottom: 1px solid var(--nile-grid-row-border, rgba(128, 128, 128, 0.14)); }
:where(nile-grid[striped]) .nile-grid-row.nile-odd { background: var(--nile-grid-stripe, rgba(128, 128, 128, 0.07)); }
:where(nile-grid) .nile-grid-row:hover { background: var(--nile-grid-hover, rgba(128, 128, 128, 0.14)); }
:where(nile-grid) .nile-grid-row.nile-selected { background: var(--nile-grid-selected-bg, Highlight); color: var(--nile-grid-selected-color, HighlightText); }
:where(nile-grid) > .nile-grid-body:focus .nile-grid-row.nile-active { outline: 1px dotted var(--nile-grid-focus, currentColor); outline-offset: -2px; }
:where(nile-grid) .nile-grid-cell { padding: 0 10px; box-sizing: border-box; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
:where(nile-grid) .nile-grid-empty { position: absolute; left: 0; right: 0; top: 0; bottom: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; pointer-events: none; }
:where(nile-grid) .nile-grid-empty b { font-size: 1.6em; opacity: 0.45; }
:where(nile-grid) .nile-grid-empty span { opacity: 0.55; }
`;

const OVERSCAN = 6;
const isBlank = (v) => v === null || v === undefined || v === "";

/**
 * A sort key for a value, built once per row: numbers and dates numerically, booleans as 0/1, text as the
 * lower-cased pieces around its digit runs (so "item 2" sorts before "item 10").
 */
function sortKey(v) {
  if (typeof v === "number") return v;
  if (v instanceof Date) return v.getTime();
  if (typeof v === "boolean") return v ? 1 : 0;
  const parts = String(v).toLowerCase().split(/(\d+)/);
  for (let i = 1; i < parts.length; i += 2) parts[i] = Number(parts[i]);
  return parts;
}

function keyCompare(x, y) {
  if (typeof x === "number" && typeof y === "number") return x - y;
  if (typeof x === "number") return -1;
  if (typeof y === "number") return 1;
  const n = Math.min(x.length, y.length);
  for (let i = 0; i < n; i++) {
    if (x[i] === y[i]) continue;
    if (i % 2 === 1) return x[i] - y[i];
    return x[i] < y[i] ? -1 : 1;
  }
  return x.length - y.length;
}

function normalize(def) {
  const col = Object.assign({}, def);
  col.key = col.key === undefined ? "" : String(col.key);
  col.title = col.title === undefined ? col.key : String(col.title);
  col.minWidth = Number.isFinite(Number(col.minWidth)) && col.minWidth !== null ? Number(col.minWidth) : 60;
  col._w = Number.isFinite(Number(col.width)) && col.width !== null && col.width !== undefined && col.width !== "" ? Number(col.width) : null;
  col.align = col.align || (col.type === "number" ? "right" : "left");
  col.sortable = col.sortable !== false;
  col.resizable = col.resizable !== false;
  return col;
}

function fromElement(el) {
  return {
    key: el.getAttribute("key") || "",
    title: el.getAttribute("title") || undefined,
    width: el.getAttribute("width") === null ? undefined : Number(el.getAttribute("width")),
    minWidth: el.getAttribute("min-width") === null ? undefined : Number(el.getAttribute("min-width")),
    align: el.getAttribute("align") || undefined,
    type: el.getAttribute("type") || undefined,
    sortable: flagAttr(el, "sortable", true),
    resizable: flagAttr(el, "resizable", true),
  };
}

export function defineGrid(tag) {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  const name = tag || "nile-grid";
  if (customElements.get && customElements.get(name)) return;
  style("grid", CSS);

  class NileGrid extends HTMLElement {
    static get observedAttributes() {
      return ["row-height", "select", "empty", "empty-hint", "sort", "key"];
    }
    get _st() {
      return this._state || (this._state = { rows: [], columns: [], view: null, sel: new Map(), els: new Map(), pool: [], pos: null, active: -1, anchor: -1, sort: null, built: false, queued: false, shown: {}, scrollX: 0 });
    }
    connectedCallback() {
      this._build();
    }
    disconnectedCallback() {
      const st = this._st;
      if (st.watch) st.watch.disconnect();
      st.watch = null;
    }
    attributeChangedCallback(attr) {
      const st = this._st;
      if (!st.built) return;
      if (attr === "sort") {
        st.sort = this._parseSort();
        this._reindex();
        this._header();
        this._paint(true);
      } else if (attr === "empty" || attr === "empty-hint") this._emptyText();
      else {
        this._dropRows();
        this._paint(true);
      }
    }

    // ---- data --------------------------------------------------------------------------------------------
    get rows() {
      return this._st.rows;
    }
    set rows(value) {
      const st = this._st;
      st.rows = Array.isArray(value) ? value : [];
      const before = st.sel.size;
      const keep = new Map();
      if (this.getAttribute("key") && st.sel.size) {
        // The selection follows the records' keys across a replaced list.
        for (const row of st.rows) {
          const id = this._rid(row);
          if (st.sel.has(id)) keep.set(id, row);
        }
      }
      st.sel = keep;
      st.active = Math.min(st.active, st.rows.length - 1);
      this._reindex();
      this._paint(true);
      if (st.sel.size !== before) this._reportSelection();
    }
    get columns() {
      return this._st.columns;
    }
    set columns(defs) {
      const st = this._st;
      st.columns = (Array.isArray(defs) ? defs : []).map(normalize);
      if (st.built) {
        this._dropRows();
        this._header();
        this._paint(true);
      }
    }
    get rowCount() {
      return this._count();
    }
    /** The selected records, in the order they appear in the grid. */
    get selected() {
      return this._selectedIndexes().map((i) => this._rowAt(i));
    }
    get selectedCount() {
      return this._st.sel.size;
    }
    /** How many rows are currently drawn (a few more than fit on screen, whatever the size of the data). */
    get mountedCount() {
      return this._st.els.size;
    }
    get selectedIndex() {
      const at = this._selectedIndexes();
      return at.length ? at[0] : -1;
    }
    get sortKey() {
      return this._st.sort ? this._st.sort.key : null;
    }
    get sortDirection() {
      return this._st.sort ? this._st.sort.dir : null;
    }
    add(row, index) {
      const rows = this._st.rows;
      if (Number.isInteger(index) && index >= 0 && index < rows.length) rows.splice(index, 0, row);
      else rows.push(row);
      this._reindex();
      this._paint(true);
      return this;
    }
    remove(target) {
      const st = this._st;
      const at = typeof target === "number" ? (st.view ? st.view[target] : target) : st.rows.indexOf(target);
      if (at === undefined || at < 0 || at >= st.rows.length) return this;
      const had = st.sel.delete(this._rid(st.rows[at]));
      st.rows.splice(at, 1);
      this._reindex();
      this._paint(true);
      if (had) this._reportSelection();
      return this;
    }
    update(index, patch) {
      const st = this._st;
      const at = st.view ? st.view[index] : index;
      if (at === undefined || !st.rows[at]) return this;
      Object.assign(st.rows[at], patch);
      this._reindex();
      this._paint(true);
      return this;
    }
    clear() {
      this.rows = [];
      return this;
    }
    refresh() {
      this._reindex();
      this._paint(true);
      return this;
    }
    select(...targets) {
      const st = this._st;
      st.sel.clear();
      for (const t of targets.flat()) {
        const row = typeof t === "number" ? this._rowAt(t) : t;
        if (row) st.sel.set(this._rid(row), row);
      }
      this._paintSelection();
      this._reportSelection();
      return this;
    }
    sortBy(key, dir) {
      const st = this._st;
      st.sort = key === null || key === undefined ? null : { key: String(key), dir: dir === "desc" ? "desc" : "asc" };
      this._reindex();
      this._header();
      this._paint(true);
      return this;
    }
    /** Where each selected record is in the grid (ascending), without scanning every row unless a sort changed. */
    _selectedIndexes() {
      const st = this._st;
      if (!st.sel.size) return [];
      if (!st.pos) {
        st.pos = new Map();
        const n = this._count();
        for (let i = 0; i < n; i++) st.pos.set(this._rid(this._rowAt(i)), i);
      }
      const out = [];
      for (const id of st.sel.keys()) {
        const at = st.pos.get(id);
        if (at !== undefined) out.push(at);
      }
      return out.sort((a, b) => a - b);
    }
    scrollToRow(index) {
      this._reveal(Math.max(0, Math.min(this._count() - 1, index)), true);
    }

    // ---- helpers -----------------------------------------------------------------------------------------
    _rid(row) {
      const key = this.getAttribute("key");
      return key ? row[key] : row;
    }
    _count() {
      const st = this._st;
      return st.view ? st.view.length : st.rows.length;
    }
    _rowAt(vi) {
      const st = this._st;
      return st.rows[st.view ? st.view[vi] : vi];
    }
    _rowH() {
      return Math.max(16, numberAttr(this, "row-height", 32));
    }
    _mode() {
      const m = this.getAttribute("select");
      return m === "none" || m === "multiple" ? m : "single";
    }
    _parseSort() {
      const v = this.getAttribute("sort");
      if (!v) return null;
      const [key, dir] = v.split(":");
      return { key: key, dir: dir === "desc" ? "desc" : "asc" };
    }
    _col(key) {
      return this._st.columns.find((c) => c.key === key);
    }
    _value(row, col) {
      return col.value ? col.value(row) : row[col.key];
    }
    _reindex() {
      const st = this._st;
      st.pos = null;
      const sort = st.sort;
      const col = sort && this._col(sort.key);
      if (!col) {
        st.view = null;
        return;
      }
      const sign = sort.dir === "desc" ? -1 : 1;
      // A column's own `compare(a, b)` sees the raw values; otherwise keys are computed once up front.
      const values = st.rows.map((r) => this._value(r, col));
      const keys = col.compare ? values : values.map((v) => (isBlank(v) ? null : sortKey(v)));
      const cmp = col.compare || keyCompare;
      const order = st.rows.map((_, i) => i);
      order.sort((a, b) => {
        const x = keys[a], y = keys[b];
        const blankX = col.compare ? isBlank(x) : x === null, blankY = col.compare ? isBlank(y) : y === null;
        // Blank values go last, whichever way the column is sorted.
        if (blankX || blankY) return blankX && blankY ? 0 : blankX ? 1 : -1;
        return sign * cmp(x, y);
      });
      st.view = order;
    }

    // ---- DOM ---------------------------------------------------------------------------------------------
    _build() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      const declared = Array.from(this.children).filter((c) => c.tagName && c.tagName.toLowerCase() === "nile-column");
      if (declared.length && !st.columns.length) st.columns = declared.map((c) => normalize(fromElement(c)));
      declared.forEach((c) => c.remove());
      if (!st.sort) st.sort = this._parseSort();
      this.setAttribute("role", "grid");
      const head = document.createElement("div");
      head.className = "nile-grid-head";
      const hrow = document.createElement("div");
      hrow.className = "nile-grid-hrow";
      hrow.setAttribute("role", "row");
      head.appendChild(hrow);
      const body = document.createElement("div");
      body.className = "nile-grid-body";
      body.setAttribute("tabindex", "0");
      const spacer = document.createElement("div");
      spacer.className = "nile-grid-spacer";
      const empty = document.createElement("div");
      empty.className = "nile-grid-empty";
      body.appendChild(spacer);
      body.appendChild(empty);
      this.appendChild(head);
      this.appendChild(body);
      Object.assign(st, { head, hrow, body, spacer, empty });
      body.addEventListener("scroll", () => {
        if (body.scrollLeft !== st.scrollX) {
          st.scrollX = body.scrollLeft;
          hrow.style.transform = "translateX(" + -st.scrollX + "px)";
        }
        this._queue();
      });
      body.addEventListener("click", (e) => this._click(e));
      body.addEventListener("dblclick", (e) => {
        const row = e.target.closest && e.target.closest(".nile-grid-row");
        if (row) this._activate(row._vi);
      });
      body.addEventListener("keydown", (e) => this._onKey(e));
      if (typeof ResizeObserver !== "undefined") {
        st.watch = new ResizeObserver(() => this._queue());
        st.watch.observe(body);
      }
      this._emptyText();
      this._reindex();
      this._header();
      this._paint(true);
    }
    _queue() {
      const st = this._st;
      if (st.queued) return;
      st.queued = true;
      requestAnimationFrame(() => {
        st.queued = false;
        this._paint(false);
      });
    }
    _emptyText() {
      const st = this._st;
      if (!st.empty) return;
      st.empty.innerHTML = "";
      const title = this.getAttribute("empty");
      const hint = this.getAttribute("empty-hint");
      if (title) {
        const b = document.createElement("b");
        b.textContent = title;
        st.empty.appendChild(b);
      }
      if (hint) {
        const s = document.createElement("span");
        s.textContent = hint;
        st.empty.appendChild(s);
      }
    }
    _template() {
      return this._st.columns.map((c) => (c._w ? c._w + "px" : "minmax(" + c.minWidth + "px, 1fr)")).join(" ");
    }
    _minWidth() {
      return this._st.columns.reduce((sum, c) => sum + (c._w || c.minWidth), 0);
    }
    _header() {
      const st = this._st;
      if (!st.hrow) return;
      st.hrow.innerHTML = "";
      st.hrow.style.gridTemplateColumns = this._template();
      st.hrow.style.minWidth = this._minWidth() + "px";
      st.columns.forEach((col) => {
        const cell = document.createElement("div");
        cell.className = "nile-grid-hcell";
        cell.setAttribute("role", "columnheader");
        cell.style.justifyContent = col.align === "right" ? "flex-end" : col.align === "center" ? "center" : "flex-start";
        const title = document.createElement("span");
        title.className = "nile-grid-title";
        title.textContent = col.title;
        cell.appendChild(title);
        if (st.sort && st.sort.key === col.key) {
          const mark = document.createElement("span");
          mark.className = "nile-grid-sort";
          mark.textContent = st.sort.dir === "desc" ? "↓" : "↑";
          cell.appendChild(mark);
          cell.setAttribute("aria-sort", st.sort.dir === "desc" ? "descending" : "ascending");
        }
        cell.addEventListener("click", () => {
          if (st.justResized) {
            st.justResized = false;
            return;
          }
          if (col.sortable) this._cycle(col);
        });
        if (col.resizable) {
          const handle = document.createElement("div");
          handle.className = "nile-grid-resizer";
          handle.addEventListener("mousedown", (e) => this._resize(e, col, cell));
          cell.appendChild(handle);
        }
        st.hrow.appendChild(cell);
      });
    }
    _cycle(col) {
      const st = this._st;
      if (!st.sort || st.sort.key !== col.key) st.sort = { key: col.key, dir: "asc" };
      else if (st.sort.dir === "asc") st.sort = { key: col.key, dir: "desc" };
      else st.sort = null;
      this._reindex();
      this._header();
      this._paint(true);
      emit(this, "sort", { key: st.sort ? st.sort.key : null, direction: st.sort ? st.sort.dir : null }, false);
    }
    _resize(e, col, cell) {
      const st = this._st;
      const start = e.clientX;
      const base = cell.getBoundingClientRect().width;
      let moved = false;
      const move = (m) => {
        const w = Math.max(col.minWidth, Math.round(base + (m.clientX - start)));
        if (Math.abs(m.clientX - start) > 2) moved = true;
        col._w = w;
        this._applyTemplate();
      };
      const up = () => {
        document.removeEventListener("mousemove", move);
        document.removeEventListener("mouseup", up);
        if (moved) {
          st.justResized = true;
          emit(this, "columnresize", { key: col.key, width: col._w }, false);
        }
      };
      document.addEventListener("mousemove", move);
      document.addEventListener("mouseup", up);
      e.preventDefault();
      e.stopPropagation();
    }
    _applyTemplate() {
      const st = this._st;
      const template = this._template();
      st.hrow.style.gridTemplateColumns = template;
      st.hrow.style.minWidth = this._minWidth() + "px";
      st.spacer.style.minWidth = this._minWidth() + "px";
      for (const el of st.els.values()) el.style.gridTemplateColumns = template;
      for (const el of st.pool) el.style.gridTemplateColumns = template;
    }
    _fill(cell, col, row, vi) {
      const raw = this._value(row, col);
      const out = col.render ? col.render(row, raw, vi, cell) : col.format ? col.format(raw, row) : raw;
      if (out && typeof out === "object" && out.nodeType) cell.appendChild(out);
      else if (out && typeof out === "object" && "html" in out) cell.innerHTML = out.html;
      else cell.textContent = isBlank(out) ? "" : String(out);
    }
    /** Take a row element from the pool (or build one) and show record `vi` in it. */
    _makeRow(vi) {
      const st = this._st;
      let el = st.pool.pop();
      if (el) {
        el.style.display = "";
      } else {
        el = document.createElement("div");
        el.setAttribute("role", "row");
        el.style.height = this._rowH() + "px";
        el.style.gridTemplateColumns = this._template();
        st.columns.forEach((col) => {
          const cell = document.createElement("div");
          cell.className = "nile-grid-cell";
          cell.setAttribute("role", "gridcell");
          cell.style.textAlign = col.align;
          el.appendChild(cell);
        });
      }
      const row = this._rowAt(vi);
      el._vi = vi;
      el.setAttribute("aria-rowindex", String(vi + 1));
      el.style.top = vi * this._rowH() + "px";
      st.columns.forEach((col, c) => {
        const cell = el.children[c];
        if (cell.firstChild && cell.firstChild.nodeType === 1) cell.textContent = "";
        this._fill(cell, col, row, vi);
      });
      const selected = st.sel.has(this._rid(row));
      el.className = "nile-grid-row" + (vi % 2 ? " nile-odd" : "") + (selected ? " nile-selected" : "") + (vi === st.active ? " nile-active" : "");
      if (selected) el.setAttribute("aria-selected", "true");
      else if (el.hasAttribute("aria-selected")) el.removeAttribute("aria-selected");
      return el;
    }
    /** Remove every row element (the columns or the row height changed). */
    _dropRows() {
      const st = this._st;
      for (const el of st.els.values()) el.remove();
      for (const el of st.pool) el.remove();
      st.els.clear();
      st.pool = [];
    }
    _mark(el, vi, row) {
      const st = this._st;
      const selected = st.sel.has(this._rid(row));
      if (selected) el.classList.add("nile-selected");
      else el.classList.remove("nile-selected");
      if (vi === st.active) el.classList.add("nile-active");
      else el.classList.remove("nile-active");
      if (selected) el.setAttribute("aria-selected", "true");
      else el.removeAttribute("aria-selected");
    }
    _paintSelection() {
      const st = this._st;
      for (const [vi, el] of st.els) this._mark(el, vi, this._rowAt(vi));
    }
    /** Show the rows in view (all of them when `force`, e.g. after the data changed); rows leaving the view are recycled. */
    _paint(force) {
      const st = this._st;
      if (!st.built) return;
      const n = this._count();
      const h = this._rowH();
      // Write to the DOM only when something changed: every write makes the page lay out again.
      const total = n * h + "px";
      if (st.shown.height !== total) {
        st.spacer.style.height = total;
        st.shown.height = total;
      }
      const minWidth = this._minWidth() + "px";
      if (st.shown.minWidth !== minWidth) {
        st.spacer.style.minWidth = minWidth;
        st.shown.minWidth = minWidth;
      }
      const empty = n ? "none" : "flex";
      if (st.shown.empty !== empty) {
        st.empty.style.display = empty;
        st.shown.empty = empty;
      }
      const top = st.body.scrollTop;
      const view = st.body.clientHeight || 0;
      const first = Math.max(0, Math.floor(top / h) - OVERSCAN);
      const last = Math.min(n, Math.ceil((top + view) / h) + OVERSCAN);
      for (const [vi, el] of Array.from(st.els)) {
        if (force || vi < first || vi >= last) {
          el.style.display = "none";
          st.els.delete(vi);
          st.pool.push(el);
        }
      }
      for (let vi = first; vi < last; vi++) {
        if (st.els.has(vi)) continue;
        const reused = st.pool.length > 0;
        const el = this._makeRow(vi);
        if (!reused) st.spacer.appendChild(el);
        st.els.set(vi, el);
      }
    }

    // ---- selection and keys ------------------------------------------------------------------------------
    _click(e) {
      const rowEl = e.target.closest && e.target.closest(".nile-grid-row");
      if (!rowEl) return;
      if (e.target.closest("button, a, input, select, textarea, [data-grid-ignore]")) return;
      this._pick(rowEl._vi, e.shiftKey, e.ctrlKey || e.metaKey);
    }
    _pick(vi, shift, toggle) {
      const st = this._st;
      const mode = this._mode();
      st.active = vi;
      if (mode === "none") {
        this._paintSelection();
        return;
      }
      const add = (i) => {
        const row = this._rowAt(i);
        st.sel.set(this._rid(row), row);
      };
      if (mode === "multiple" && shift && st.anchor >= 0) {
        st.sel.clear();
        for (let i = Math.min(st.anchor, vi); i <= Math.max(st.anchor, vi); i++) add(i);
      } else if (mode === "multiple" && toggle) {
        const id = this._rid(this._rowAt(vi));
        if (st.sel.has(id)) st.sel.delete(id);
        else add(vi);
        st.anchor = vi;
      } else {
        st.sel.clear();
        add(vi);
        st.anchor = vi;
      }
      this._paintSelection();
      this._reportSelection();
    }
    _reportSelection() {
      const indexes = this._selectedIndexes();
      emit(this, "selectionchange", { selected: indexes.map((i) => this._rowAt(i)), indexes: indexes }, false);
    }
    _activate(vi) {
      const row = this._rowAt(vi);
      if (row) emit(this, "rowactivate", { row: row, index: vi }, false);
    }
    _reveal(vi, top) {
      const st = this._st;
      const h = this._rowH();
      const view = st.body.clientHeight;
      const y = vi * h;
      if (top || y < st.body.scrollTop) st.body.scrollTop = y;
      else if (y + h > st.body.scrollTop + view) st.body.scrollTop = y + h - view;
      this._paint(false);
    }
    _onKey(e) {
      const st = this._st;
      const n = this._count();
      if (!n) return;
      const page = Math.max(1, Math.floor(st.body.clientHeight / this._rowH()) - 1);
      const from = st.active >= 0 ? st.active : 0;
      let to = null;
      switch (e.key) {
        case "ArrowDown": to = st.active < 0 ? 0 : from + 1; break;
        case "ArrowUp": to = st.active < 0 ? 0 : from - 1; break;
        case "PageDown": to = from + page; break;
        case "PageUp": to = from - page; break;
        case "Home": to = 0; break;
        case "End": to = n - 1; break;
        case "Enter":
          if (st.active >= 0) this._activate(st.active);
          e.preventDefault();
          return;
        case " ":
          if (st.active >= 0 && this._mode() === "multiple") this._pick(st.active, false, true);
          e.preventDefault();
          return;
        case "a":
        case "A":
          if ((e.ctrlKey || e.metaKey) && this._mode() === "multiple") {
            st.sel.clear();
            for (let i = 0; i < n; i++) {
              const row = this._rowAt(i);
              st.sel.set(this._rid(row), row);
            }
            this._paintSelection();
            this._reportSelection();
            e.preventDefault();
          }
          return;
        default: return;
      }
      to = Math.max(0, Math.min(n - 1, to));
      this._pick(to, e.shiftKey, false);
      this._reveal(to, false);
      e.preventDefault();
    }
  }
  customElements.define(name, NileGrid);
}

defineGrid();
