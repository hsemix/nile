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
// `sort` (detail: { key, direction }), `columnresize` (detail: { key, width }), and for editing `beforecelledit`
// (cancelable), `cellchange` (detail: { row, index, key, value, oldValue }), `celleditcancel`.
// Keys: arrows, Page Up/Down, Home/End move; Shift extends (select=multiple); Space toggles, Ctrl/Cmd+A selects all.
// Column headers are focusable: Left/Right move between them, Enter or Space sorts, Shift+Left/Right makes the column
// narrower or wider by 10px (Ctrl+Shift: 50px), Home/End jump to the first/last header.
//
// Cell editing: give a column `editable: true` (or `editable(row) -> boolean`; `editable` attribute on <nile-column>).
// The grid then has an active cell (Left/Right move it, a click sets it). F2, Enter, a double-click, or typing a
// character starts editing; Enter commits and moves down, Tab commits and moves to the next editable cell, Escape
// cancels, leaving the cell commits. Column fields: `editor` ("text" default, "number", "select", "checkbox", or
// a function `(row, value, cell) -> { element, value() }`), `options` (select: strings or { value, label }), `parse(text, row)`,
// `validate(value, row)` (false or an error text keeps the editor open and marks it invalid), `set(row, value)` (to
// write somewhere other than `row[key]`). `type: "boolean"` columns show a check box that Space or a click toggles.
// Computed columns are refreshed after a commit. Methods: edit(index, key), commitEdit(), cancelEdit(); `editing` is
// { index, key } or null.

import { style, emit, numberAttr, flagAttr } from "@nile/ui/base";

const CSS = `
:where(nile-grid) { display: flex; flex-direction: column; height: 300px; min-width: 0; min-height: 0; box-sizing: border-box; overflow: hidden;
  border: 1px solid var(--nile-grid-border, rgba(128, 128, 128, 0.35)); background: var(--nile-grid-bg, Canvas); color: var(--nile-grid-color, CanvasText); }
:where(nile-grid) > .nile-grid-head { flex: none; overflow: hidden; background: var(--nile-grid-head-bg, rgba(128, 128, 128, 0.14)); border-bottom: 1px solid var(--nile-grid-border, rgba(128, 128, 128, 0.35)); }
:where(nile-grid[header="false"]) > .nile-grid-head { display: none; }
:where(nile-grid) .nile-grid-hrow { display: grid; height: var(--nile-grid-head-height, 34px); grid-template-columns: var(--nile-grid-cols); min-width: var(--nile-grid-min); }
:where(nile-grid) .nile-grid-hcell { position: relative; display: flex; align-items: center; gap: 4px; padding: 0 10px; box-sizing: border-box; overflow: hidden; white-space: nowrap; font-weight: 600; user-select: none; }
:where(nile-grid) .nile-grid-hcell > .nile-grid-title { overflow: hidden; text-overflow: ellipsis; }
:where(nile-grid) .nile-grid-hcell > .nile-grid-sort { flex: none; opacity: 0.7; }
:where(nile-grid) .nile-grid-resizer { position: absolute; top: 0; bottom: 0; right: 0; width: 6px; cursor: col-resize; }
:where(nile-grid) .nile-grid-hcell:focus { outline: 2px solid var(--nile-grid-focus, #2563eb); outline-offset: -2px; }
:where(nile-grid) > .nile-grid-body { flex: 1; min-height: 0; overflow: auto; position: relative; outline: none; }
:where(nile-grid) .nile-grid-spacer { position: relative; min-width: var(--nile-grid-min); }
:where(nile-grid) .nile-grid-row { position: absolute; left: 0; right: 0; display: grid; grid-template-columns: var(--nile-grid-cols); align-items: center; box-sizing: border-box; border-bottom: 1px solid var(--nile-grid-row-border, rgba(128, 128, 128, 0.14)); }
:where(nile-grid[striped]) .nile-grid-row.nile-odd { background: var(--nile-grid-stripe, rgba(128, 128, 128, 0.07)); }
:where(nile-grid) .nile-grid-row:hover { background: var(--nile-grid-hover, rgba(128, 128, 128, 0.14)); }
:where(nile-grid) .nile-grid-row.nile-selected { background: var(--nile-grid-selected-bg, Highlight); color: var(--nile-grid-selected-color, HighlightText); }
:where(nile-grid) > .nile-grid-body:focus .nile-grid-row.nile-active { outline: 1px dotted var(--nile-grid-focus, currentColor); outline-offset: -2px; }
:where(nile-grid) .nile-grid-cell { padding: 0 10px; box-sizing: border-box; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
:where(nile-grid) > .nile-grid-body:focus .nile-grid-cell.nile-active-cell { outline: 2px solid var(--nile-grid-focus, #2563eb); outline-offset: -2px; }
:where(nile-grid) .nile-grid-cell.nile-editing { padding: 0; overflow: visible; }
:where(nile-grid) .nile-grid-editor { width: 100%; height: 100%; box-sizing: border-box; margin: 0; padding: 0 9px; font: inherit; color: var(--nile-grid-color, CanvasText); background: var(--nile-grid-bg, Canvas);
  border: 2px solid var(--nile-grid-focus, #2563eb); border-radius: 0; outline: none; box-shadow: none; }
:where(nile-grid) .nile-grid-editor.nile-invalid { border-color: var(--nile-grid-invalid, #dc2626); }
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
  col.editable = col.editable === true || typeof col.editable === "function" ? col.editable : false;
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
    editable: el.hasAttribute("editable") ? flagAttr(el, "editable", true) : undefined,
    editor: el.getAttribute("editor") || undefined,
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
      return this._state || (this._state = { rows: [], columns: [], view: null, sel: new Map(), els: new Map(), pool: [], pos: null, active: -1, anchor: -1, sort: null, built: false, queued: false, shown: {}, scrollX: 0, col: 0, edit: null, hfocus: 0 });
    }
    connectedCallback() {
      this._build();
    }
    disconnectedCallback() {
      const st = this._st;
      if (st.drag) st.drag();
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
      this._endEdit(false);
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
      this._endEdit(false);
      st.col = 0;
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
        if (e.target.closest && e.target.closest(".nile-grid-editor")) return;
        const row = e.target.closest && e.target.closest(".nile-grid-row");
        if (!row) return;
        const c = this._cellOf(e.target, row);
        if (c >= 0 && this._editableAt(row._vi, c) && this._beginEdit(row._vi, c)) return;
        this._activate(row._vi);
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
      const hadFocus = st.hrow.contains(document.activeElement);
      st.hrow.innerHTML = "";
      this._applyTemplate();
      st.hfocus = Math.max(0, Math.min(st.columns.length - 1, st.hfocus));
      st.columns.forEach((col, c) => {
        const cell = document.createElement("div");
        cell.className = "nile-grid-hcell";
        cell.setAttribute("role", "columnheader");
        cell.setAttribute("tabindex", c === st.hfocus ? "0" : "-1");
        cell.addEventListener("keydown", (e) => this._headKey(e, col, c));
        cell.addEventListener("focus", () => this._headFocus(c));
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
      if (hadFocus && st.hrow.children[st.hfocus]) st.hrow.children[st.hfocus].focus();
    }
    _headFocus(c) {
      const st = this._st;
      if (st.hfocus === c) return;
      const old = st.hrow.children[st.hfocus];
      if (old) old.setAttribute("tabindex", "-1");
      st.hfocus = c;
      if (st.hrow.children[c]) st.hrow.children[c].setAttribute("tabindex", "0");
    }
    _headKey(e, col, c) {
      const st = this._st;
      const last = st.columns.length - 1;
      let to = -1;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        const dir = e.key === "ArrowLeft" ? -1 : 1;
        if (e.shiftKey) {
          if (col.resizable) this._resizeBy(col, dir * (e.ctrlKey || e.metaKey ? 50 : 10), st.hrow.children[c]);
          e.preventDefault();
          return;
        }
        to = Math.max(0, Math.min(last, c + dir));
      } else if (e.key === "Home") to = 0;
      else if (e.key === "End") to = last;
      else if (e.key === "Enter" || e.key === " ") {
        if (col.sortable) this._cycle(col);
        e.preventDefault();
        return;
      } else return;
      e.preventDefault();
      if (st.hrow.children[to]) st.hrow.children[to].focus();
    }
    /** Make a column wider or narrower by `dx` px (the keyboard route to what dragging the header edge does). */
    _resizeBy(col, dx, cell) {
      const now = col._w || Math.round(cell.getBoundingClientRect().width);
      const w = Math.max(col.minWidth, now + dx);
      if (w === col._w) return;
      col._w = w;
      this._applyTemplate();
      emit(this, "columnresize", { key: col.key, width: w }, false);
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
      const base = cell.getBoundingClientRect().width; // measured once: moves are arithmetic
      let moved = false;
      let want = col._w || base;
      let queued = 0;
      // One style write per frame at most, and none when the width did not change.
      const flush = () => {
        queued = 0;
        if (col._w === want) return;
        col._w = want;
        this._applyTemplate();
      };
      const move = (m) => {
        const w = Math.max(col.minWidth, Math.round(base + (m.clientX - start)));
        if (Math.abs(m.clientX - start) > 2) moved = true;
        if (w === want) return;
        want = w;
        if (!queued) queued = requestAnimationFrame(flush);
      };
      // The drag ends on release, but also when the window loses the pointer or the grid leaves the page.
      const up = () => {
        if (st.drag !== up) return;
        st.drag = null;
        document.removeEventListener("mousemove", move);
        document.removeEventListener("mouseup", up);
        window.removeEventListener("blur", up);
        if (queued && typeof cancelAnimationFrame === "function") cancelAnimationFrame(queued);
        flush();
        if (moved) {
          st.justResized = true;
          emit(this, "columnresize", { key: col.key, width: col._w }, false);
        }
      };
      if (st.drag) st.drag();
      st.drag = up;
      document.addEventListener("mousemove", move);
      document.addEventListener("mouseup", up);
      window.addEventListener("blur", up);
      e.preventDefault();
      e.stopPropagation();
    }
    /** The column widths live in two custom properties on the grid, so changing them is one write however many rows are drawn. */
    _applyTemplate() {
      const st = this._st;
      const template = this._template();
      const min = this._minWidth() + "px";
      if (st.shown.cols !== template) {
        this.style.setProperty("--nile-grid-cols", template);
        st.shown.cols = template;
      }
      if (st.shown.min !== min) {
        this.style.setProperty("--nile-grid-min", min);
        st.shown.min = min;
      }
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
      this._markCell(el, vi);
      return el;
    }
    /** Remove every row element (the columns or the row height changed). */
    _dropRows() {
      const st = this._st;
      this._endEdit(false);
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
      this._markCell(el, vi);
    }
    /** Outline the active cell (only when the grid has editable columns, where cells are what the keys move between). */
    _markCell(el, vi) {
      const st = this._st;
      if (!this._cellMode()) return;
      for (let c = 0; c < el.children.length; c++) {
        if (vi === st.active && c === st.col) el.children[c].classList.add("nile-active-cell");
        else el.children[c].classList.remove("nile-active-cell");
      }
    }
    _cellMode() {
      return this._st.columns.some((c) => c.editable);
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
      this._applyTemplate();
      const empty = n ? "none" : "flex";
      if (st.shown.empty !== empty) {
        st.empty.style.display = empty;
        st.shown.empty = empty;
      }
      const top = st.body.scrollTop;
      const view = st.body.clientHeight || 0;
      const first = Math.max(0, Math.floor(top / h) - OVERSCAN);
      const last = Math.min(n, Math.ceil((top + view) / h) + OVERSCAN);
      // An editor belongs to its record, not to a position: the data or the sort order may have moved it.
      let keep = -1;
      const edit = st.edit;
      if (edit) {
        if (force) {
          const at = this._viewIndexOf(edit.row);
          if (at < 0) this._endEdit(false);
          else edit.vi = at;
        } else if ((edit.vi < first || edit.vi >= last) && !this._endEdit(true)) keep = edit.vi;
      }
      for (const [vi, el] of Array.from(st.els)) {
        if (vi !== keep && (force || vi < first || vi >= last)) {
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
      if (st.edit) {
        this._placeEditor();
        // The record may have moved (a sort, new data): scroll to it rather than lose the editor.
        if (force && !st.following) {
          st.following = true;
          this._reveal(st.edit.vi, false);
          st.following = false;
        }
      }
    }
    /** Where a record is in the grid right now, or -1. */
    _viewIndexOf(row) {
      const st = this._st;
      const at = st.rows.indexOf(row);
      return at < 0 || !st.view ? at : st.view.indexOf(at);
    }
    /** Put the open editor back into its record's (rebuilt) row. */
    _placeEditor() {
      const st = this._st;
      const e = st.edit;
      let el = st.els.get(e.vi);
      if (!el) {
        const reused = st.pool.length > 0;
        el = this._makeRow(e.vi);
        if (!reused) st.spacer.appendChild(el);
        st.els.set(e.vi, el);
      }
      const cell = el.children[e.c];
      if (e.cell === cell && e.input.parentNode === cell) return;
      e.cell.classList.remove("nile-editing");
      cell.textContent = "";
      cell.classList.add("nile-editing");
      cell.appendChild(e.input);
      e.cell = cell;
      e.input.focus();
    }

    // ---- selection and keys ------------------------------------------------------------------------------
    _click(e) {
      const rowEl = e.target.closest && e.target.closest(".nile-grid-row");
      if (!rowEl) return;
      if (e.target.closest("button, a, input, select, textarea, [data-grid-ignore]")) return;
      const c = this._cellOf(e.target, rowEl);
      if (c >= 0) this._st.col = c;
      this._pick(rowEl._vi, e.shiftKey, e.ctrlKey || e.metaKey);
      // A check box cell toggles on a click.
      if (c >= 0 && this._toggles(c) && this._editableAt(rowEl._vi, c)) this._toggle(rowEl._vi, c);
    }
    _cellOf(target, rowEl) {
      const cell = target.closest && target.closest(".nile-grid-cell");
      return cell ? Array.prototype.indexOf.call(rowEl.children, cell) : -1;
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
    /** Keys that work on the active cell; true when handled. */
    _cellKey(e) {
      const st = this._st;
      if (!this._cellMode() || st.active < 0) return false;
      const last = st.columns.length - 1;
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        const to = Math.max(0, Math.min(last, st.col + (e.key === "ArrowLeft" ? -1 : 1)));
        if (to !== st.col) {
          st.col = to;
          this._paintSelection();
        }
        e.preventDefault();
        return true;
      }
      const editable = this._editableAt(st.active, st.col);
      if (e.key === "Enter" && editable) {
        e.preventDefault();
        if (this._toggles(st.col)) this._toggle(st.active, st.col);
        else this._beginEdit(st.active, st.col);
        return true;
      }
      if (e.key === " " && editable && this._toggles(st.col)) {
        e.preventDefault();
        this._toggle(st.active, st.col);
        return true;
      }
      // Typing starts editing with that character.
      if (editable && !this._toggles(st.col) && e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        this._beginEdit(st.active, st.col, e.key);
        return true;
      }
      return false;
    }

    // ---- editing -----------------------------------------------------------------------------------------
    _editableAt(vi, c) {
      const col = this._st.columns[c];
      const row = this._rowAt(vi);
      if (!col || !row || !col.editable) return false;
      return col.editable === true || !!col.editable(row);
    }
    _toggles(c) {
      const col = this._st.columns[c];
      return !!col && (col.editor === "checkbox" || (col.type === "boolean" && !col.editor));
    }
    get editing() {
      const e = this._st.edit;
      return e ? { index: e.vi, key: e.col.key } : null;
    }
    /** Start editing the cell of row `index` in column `key` (the active column by default). */
    edit(index, key) {
      const st = this._st;
      const c = key === undefined ? st.col : st.columns.findIndex((col) => col.key === String(key));
      if (c < 0 || index < 0 || index >= this._count()) return this;
      this._reveal(index, false);
      st.active = index;
      st.col = c;
      this._beginEdit(index, c);
      return this;
    }
    commitEdit() {
      return this._endEdit(true);
    }
    cancelEdit() {
      this._endEdit(false);
      return this;
    }
    _toggle(vi, c) {
      const col = this._st.columns[c];
      const row = this._rowAt(vi);
      const old = this._value(row, col);
      this._store(vi, c, !old, old);
    }
    /** Write a value into the record, refresh the row and report it. */
    _store(vi, c, value, old, record) {
      const st = this._st;
      const col = st.columns[c];
      const row = record || this._rowAt(vi);
      if (value === old) return;
      if (col.set) col.set(row, value);
      else if (!col.value) row[col.key] = value;
      emit(this, "cellchange", { row: row, index: vi, key: col.key, value: value, oldValue: old }, false);
      if (st.sort && st.sort.key === col.key) {
        // The sort order may have changed: follow the record.
        this._reindex();
        const at = st.view ? st.view.indexOf(st.rows.indexOf(row)) : st.rows.indexOf(row);
        st.active = at;
        this._paint(true);
        if (at >= 0) this._reveal(at, false);
        this._reportSelection();
      } else this._refreshRow(vi);
    }
    _refreshRow(vi) {
      const st = this._st;
      const el = st.els.get(vi);
      if (!el) return;
      const row = this._rowAt(vi);
      st.columns.forEach((col, c) => {
        const cell = el.children[c];
        cell.textContent = "";
        this._fill(cell, col, row, vi);
      });
    }
    _beginEdit(vi, c, initial) {
      const st = this._st;
      if (st.edit && !this._endEdit(true)) return false;
      const col = st.columns[c];
      const row = this._rowAt(vi);
      if (!this._editableAt(vi, c)) return false;
      if (!emit(this, "beforecelledit", { row: row, index: vi, key: col.key }, true)) return false;
      this._reveal(vi, false);
      const el = st.els.get(vi);
      if (!el) return false;
      const cell = el.children[c];
      const raw = this._value(row, col);
      let input, getValue;
      const editor = col.editor || (col.type === "number" ? "number" : "text");
      if (typeof editor === "function") {
        const made = editor(row, raw, cell);
        input = made.element || made;
        getValue = made.value ? () => made.value() : () => input.value;
      } else if (editor === "select") {
        input = document.createElement("select");
        const options = (col.options || []).map((o) => (o !== null && typeof o === "object" ? o : { value: o, label: String(o) }));
        options.forEach((o) => {
          const opt = document.createElement("option");
          opt.value = String(o.value);
          opt.textContent = o.label !== undefined ? o.label : String(o.value);
          input.appendChild(opt);
        });
        input.value = String(raw);
        getValue = () => {
          const hit = options.find((o) => String(o.value) === input.value);
          return hit ? hit.value : input.value;
        };
      } else {
        input = document.createElement("input");
        input.setAttribute("type", "text");
        input.value = initial !== undefined ? initial : isBlank(raw) ? "" : String(raw);
        if (editor === "number") input.setAttribute("inputmode", "decimal");
        getValue = () => input.value;
      }
      input.classList.add("nile-grid-editor");
      cell.textContent = "";
      cell.classList.add("nile-editing");
      cell.appendChild(input);
      st.edit = { vi: vi, c: c, col: col, row: row, input: input, getValue: getValue, editor: editor, raw: raw, cell: cell, hadFocus: true };
      input.addEventListener("keydown", (e) => this._editKey(e));
      input.addEventListener("blur", () => {
        // Leaving the editor commits it (unless it was already ended, or the value is invalid and stays open).
        setTimeout(() => {
          if (st.edit && st.edit.input === input && !st.body.contains(document.activeElement)) this._endEdit(true);
        }, 0);
      });
      input.focus();
      if (input.select && initial === undefined && editor !== "select") input.select();
      return true;
    }
    _editKey(e) {
      const st = this._st;
      if (!st.edit) return;
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        this._endEdit(false);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const record = st.edit.row;
        if (this._endEdit(true)) {
          // A commit in the sorted column moves the record: carry on from where it is now.
          const here = Math.max(0, this._viewIndexOf(record));
          const to = Math.max(0, Math.min(this._count() - 1, here + (e.shiftKey ? -1 : 1)));
          this._pick(to, false, false);
          this._reveal(to, false);
        }
      } else if (e.key === "Tab") {
        e.preventDefault();
        const { row: record, c } = st.edit;
        if (!this._endEdit(true)) return;
        const vi = Math.max(0, this._viewIndexOf(record));
        const next = this._nextEditable(vi, c, e.shiftKey ? -1 : 1);
        if (next) {
          st.active = next[0];
          st.col = next[1];
          this._pick(next[0], false, false);
          this._beginEdit(next[0], next[1]);
        }
      }
    }
    /** The next (or previous) editable cell in reading order, or null. */
    _nextEditable(vi, c, dir) {
      const n = this._count();
      const cols = this._st.columns.length;
      let at = vi * cols + c + dir;
      while (at >= 0 && at < n * cols) {
        const r = Math.floor(at / cols), k = at % cols;
        if (this._editableAt(r, k) && !this._toggles(k)) return [r, k];
        at += dir;
      }
      return null;
    }
    /** Finish editing. With `commit`, parse and validate first; false means the value was refused and the editor stays. */
    _endEdit(commit) {
      const st = this._st;
      const e = st.edit;
      if (!e) return true;
      let value;
      if (commit) {
        const text = e.getValue();
        let problem = null;
        if (e.col.parse) value = e.col.parse(text, e.row);
        else if (e.editor === "number") {
          if (typeof text === "number") value = text;
          else if (String(text).trim() === "") value = null;
          else {
            value = Number(String(text).replace(/,/g, ""));
            if (!Number.isFinite(value)) problem = "Enter a number";
          }
        } else value = text;
        if (!problem && e.col.validate) {
          const verdict = e.col.validate(value, e.row);
          if (verdict === false) problem = "Invalid value";
          else if (typeof verdict === "string" && verdict) problem = verdict;
        }
        if (problem) {
          e.input.classList.add("nile-invalid");
          e.input.setAttribute("title", problem);
          e.input.setAttribute("aria-invalid", "true");
          if (e.input.focus) e.input.focus();
          return false;
        }
      }
      st.edit = null;
      const focused = st.body.contains(document.activeElement);
      e.cell.classList.remove("nile-editing");
      e.cell.textContent = "";
      this._fill(e.cell, e.col, e.row, e.vi);
      if (commit) this._store(e.vi, e.c, value, e.raw, e.row);
      else emit(this, "celleditcancel", { row: e.row, index: e.vi, key: e.col.key }, false);
      if (focused) st.body.focus();
      return true;
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
      if (st.edit) return;
      if (this._cellKey(e)) return;
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
        case "F2":
          if (st.active >= 0) this._beginEdit(st.active, st.col);
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
