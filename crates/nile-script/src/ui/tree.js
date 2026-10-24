// @nile/ui/tree: <nile-tree>, an expandable tree for hierarchies: product groups, folders, navigation.
//
//   <nile-tree id="groups" selected="2"></nile-tree>
//   tree.items = [{ id: 1, label: "Drinks", children: [{ id: 2, label: "Coffee" }, { id: 3, label: "Tea" }] },
//                 { id: 4, label: "Food", hasChildren: true }];      // hasChildren + a `load` function = lazy children
//   tree.load = (item) => fetchChildren(item.id);                  // returns an array (or a Promise of one)
//
// The same tree can be written in markup: <nile-tree><nile-node label="Drinks" id="1" expanded><nile-node label="Tea"/>
// </nile-node></nile-tree> (`label`, `id`, `expanded`, `disabled`, `icon` = HTML such as an inline <svg>).
//
// Items: { id, label, icon, children, expanded, disabled, hasChildren, data...: anything else you keep on it }. `id`
// defaults to the item's path ("0/1"); `label` is plain text unless you give a `render(item)` function that returns
// text or a node. Attributes: `selected` (an id), `indent` (px per level, 18), `row-height` (28), `filter` (text).
// Properties: items, selected (id), selectedItem, load, render, filter. Methods: expand(id), collapse(id),
// toggle(id), expandAll(), collapseAll(), select(id), reveal(id) (expands the ancestors), find(id), parentOf(id),
// add(item, parentId, index), remove(id), update(id, patch), refresh(). Events: `selectionchange` (detail: { id,
// item }), `activate` (Enter or double click), `expand` / `collapse` (detail: { id, item }), cancelable
// `beforeexpand`, `loaderror`.
//
// Keyboard (as in a desktop tree): Up/Down move, Right expands or goes to the first child, Left collapses or goes to
// the parent, Home/End, Enter activates, Space selects, `*` expands every sibling, a letter jumps to the next label
// starting with it. Style with --nile-tree-hover, --nile-tree-selected, --nile-tree-selected-color,
// --nile-tree-accent or the .nile-tree-row ([aria-selected], .nile-active), .nile-tree-toggle, .nile-tree-label classes.
//
// Every visible row is a DOM element, so a tree of a few thousand open nodes is fine; collapse the rest.

import { style, emit, numberAttr, CHEVRON } from "@nile/ui/base";

const CSS = `
:where(nile-tree) { display: block; overflow: auto; outline: none; position: relative; }
:where(nile-tree) .nile-tree-row { display: flex; align-items: center; gap: 4px; box-sizing: border-box; padding-right: 8px; cursor: default; white-space: nowrap; user-select: none; }
:where(nile-tree) .nile-tree-row:hover { background: var(--nile-tree-hover, rgba(127, 127, 127, 0.12)); }
:where(nile-tree) .nile-tree-row[aria-selected="true"] { background: var(--nile-tree-selected, rgba(37, 99, 235, 0.2)); color: var(--nile-tree-selected-color, inherit); }
:where(nile-tree:focus) .nile-tree-row.nile-active { outline: 2px solid var(--nile-tree-accent, #2563eb); outline-offset: -2px; }
:where(nile-tree) .nile-tree-row[aria-disabled="true"] { opacity: 0.45; }
:where(nile-tree) .nile-tree-toggle { flex: none; width: 16px; height: 16px; display: flex; align-items: center; justify-content: center; opacity: 0.7; }
:where(nile-tree) .nile-tree-toggle svg { width: 9px; height: 6px; transform: rotate(-90deg); }
:where(nile-tree) .nile-tree-row[aria-expanded="true"] .nile-tree-toggle svg { transform: none; }
:where(nile-tree) .nile-tree-icon { flex: none; display: flex; align-items: center; }
:where(nile-tree) .nile-tree-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
:where(nile-tree) .nile-tree-busy { flex: none; opacity: 0.6; font-size: 11px; }
:where(nile-tree) .nile-tree-empty { padding: 8px 10px; opacity: 0.6; }
`;

function plain(item) {
  return item !== null && typeof item === "object" ? item : { label: String(item) };
}

/** Item objects for the <nile-node> children of `el` (recursively). */
function fromMarkup(el) {
  return Array.from(el.children)
    .filter((c) => c.tagName === "NILE-NODE")
    .map((n) => {
      const item = { label: n.getAttribute("label") || n.getAttribute("title") || "" };
      if (n.hasAttribute("id")) item.id = n.getAttribute("id");
      if (n.hasAttribute("expanded")) item.expanded = true;
      if (n.hasAttribute("disabled")) item.disabled = true;
      if (n.hasAttribute("icon")) item.icon = n.getAttribute("icon");
      const kids = fromMarkup(n);
      if (kids.length) item.children = kids;
      return item;
    });
}

export function defineTree() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-tree")) return;
  style("tree", CSS);

  class NileNode extends HTMLElement {}

  class NileTree extends HTMLElement {
    static get observedAttributes() {
      return ["selected", "filter", "indent", "row-height"];
    }
    get _st() {
      return this._state || (this._state = { items: [], built: false, selected: null, active: null, filter: "", load: null, render: null, open: new Set(), loading: new Set(), loaded: new Set(), rows: [], typed: "", typedAt: 0 });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      if (!st.items.length) st.items = fromMarkup(this);
      Array.from(this.children).forEach((c) => {
        if (c.tagName === "NILE-NODE") c.remove();
      });
      this.setAttribute("role", "tree");
      if (!this.hasAttribute("tabindex")) this.setAttribute("tabindex", "0");
      this._prepare(st.items, "");
      if (this.getAttribute("selected") !== null && st.selected === null) st.selected = this._byText(this.getAttribute("selected"));
      this.addEventListener("click", (e) => this._click(e));
      this.addEventListener("dblclick", (e) => this._dblclick(e));
      this.addEventListener("keydown", (e) => this._onKey(e));
      this._paint();
    }
    attributeChangedCallback(attr, was, now) {
      const st = this._st;
      if (!st.built || was === now) return;
      if (attr === "selected") this.select(this._byText(now));
      else if (attr === "filter") this.filter = now || "";
      else this._paint();
    }

    // ---- data --------------------------------------------------------------------------------------------
    get items() {
      return this._st.items;
    }
    set items(list) {
      const st = this._st;
      st.items = Array.isArray(list) ? list : [];
      st.open = new Set();
      st.loaded = new Set();
      if (st.built) {
        this._prepare(st.items, "");
        this._paint();
      }
    }
    get load() {
      return this._st.load;
    }
    set load(fn) {
      this._st.load = typeof fn === "function" ? fn : null;
    }
    get render() {
      return this._st.render;
    }
    set render(fn) {
      this._st.render = typeof fn === "function" ? fn : null;
      if (this._st.built) this._paint();
    }
    get filter() {
      return this._st.filter;
    }
    set filter(text) {
      this._st.filter = String(text || "");
      if (this._st.built) this._paint();
    }
    get selected() {
      return this._st.selected;
    }
    set selected(id) {
      this.select(id);
    }
    get selectedItem() {
      const st = this._st;
      return st.selected === null ? null : this.find(st.selected);
    }
    /** Give every item an id (its path when it has none) and remember its parent and depth. */
    _prepare(items, path, parent, depth) {
      const st = this._st;
      items.forEach((item, i) => {
        if (item === null || typeof item !== "object") items[i] = item = plain(item);
        const here = path ? path + "/" + i : String(i);
        if (item.id === undefined || item.id === null) item.id = here;
        Object.defineProperty(item, "_parent", { value: parent || null, enumerable: false, configurable: true, writable: true });
        Object.defineProperty(item, "_depth", { value: depth || 0, enumerable: false, configurable: true, writable: true });
        if (item.expanded) st.open.add(item.id);
        if (item.children) this._prepare(item.children, here, item, (depth || 0) + 1);
      });
    }
    _byText(text) {
      if (text === null || text === undefined) return null;
      const hit = this.find(text);
      if (hit) return hit.id;
      const n = Number(text);
      return Number.isNaN(n) ? text : this.find(n) ? n : text;
    }
    find(id) {
      const walk = (items) => {
        for (const item of items) {
          if (item.id === id || String(item.id) === String(id)) return item;
          if (item.children) {
            const hit = walk(item.children);
            if (hit) return hit;
          }
        }
        return null;
      };
      return walk(this._st.items);
    }
    parentOf(id) {
      const item = this.find(id);
      return item ? item._parent : null;
    }
    _hasKids(item) {
      return (item.children && item.children.length > 0) || (!!item.hasChildren && !this._st.loaded.has(item.id) && !!this._st.load);
    }
    add(item, parentId, index) {
      const st = this._st;
      const parent = parentId === undefined || parentId === null ? null : this.find(parentId);
      const list = parent ? parent.children || (parent.children = []) : st.items;
      const at = Number.isInteger(index) && index >= 0 && index <= list.length ? index : list.length;
      list.splice(at, 0, plain(item));
      this._prepare(st.items, "");
      this._paint();
      return this;
    }
    remove(id) {
      const st = this._st;
      const item = this.find(id);
      if (!item) return this;
      const list = item._parent ? item._parent.children : st.items;
      list.splice(list.indexOf(item), 1);
      if (item._parent && !list.length) delete item._parent.children;
      if (st.selected !== null && this._within(this.find(st.selected), item)) st.selected = null;
      this._prepare(st.items, "");
      this._paint();
      return this;
    }
    update(id, patch) {
      const item = this.find(id);
      if (!item) return this;
      Object.assign(item, patch);
      this._prepare(this._st.items, "");
      this._paint();
      return this;
    }
    _within(item, ancestor) {
      for (let x = item; x; x = x._parent) if (x === ancestor) return true;
      return false;
    }
    refresh() {
      this._prepare(this._st.items, "");
      this._paint();
      return this;
    }

    // ---- expanding ---------------------------------------------------------------------------------------
    isExpanded(id) {
      return this._st.open.has(id);
    }
    expand(id) {
      const st = this._st;
      const item = this.find(id);
      if (!item || item.disabled || !this._hasKids(item) || st.open.has(item.id)) return this;
      if (!emit(this, "beforeexpand", { id: item.id, item: item }, true)) return this;
      if (!item.children && st.load) {
        if (st.loading.has(item.id)) return this;
        st.loading.add(item.id);
        this._paint();
        Promise.resolve()
          .then(() => st.load(item))
          .then((kids) => {
            st.loading.delete(item.id);
            st.loaded.add(item.id);
            item.children = Array.isArray(kids) ? kids : [];
            this._prepare(st.items, "");
            st.open.add(item.id);
            this._paint();
            emit(this, "expand", { id: item.id, item: item }, false);
          })
          .catch((err) => {
            st.loading.delete(item.id);
            this._paint();
            emit(this, "loaderror", { id: item.id, item: item, error: err }, false);
          });
        return this;
      }
      st.open.add(item.id);
      this._paint();
      emit(this, "expand", { id: item.id, item: item }, false);
      return this;
    }
    collapse(id) {
      const st = this._st;
      const item = this.find(id);
      if (!item || !st.open.has(item.id)) return this;
      st.open.delete(item.id);
      // A collapsed branch can't hide the selection: it moves to the branch itself.
      const sel = st.selected === null ? null : this.find(st.selected);
      if (sel && sel !== item && this._within(sel, item)) this.select(item.id);
      if (st.active !== null && this._within(this.find(st.active), item) && st.active !== item.id) st.active = item.id;
      this._paint();
      emit(this, "collapse", { id: item.id, item: item }, false);
      return this;
    }
    toggle(id) {
      return this.isExpanded(id) ? this.collapse(id) : this.expand(id);
    }
    expandAll() {
      const walk = (items) =>
        items.forEach((item) => {
          if (item.children && item.children.length) {
            this._st.open.add(item.id);
            walk(item.children);
          }
        });
      walk(this._st.items);
      this._paint();
      return this;
    }
    collapseAll() {
      this._st.open.clear();
      const sel = this.selectedItem;
      if (sel && sel._parent) {
        let top = sel;
        while (top._parent) top = top._parent;
        this.select(top.id);
      }
      this._paint();
      return this;
    }
    /** Expand the ancestors of `id` and scroll to it. */
    reveal(id) {
      const item = this.find(id);
      if (!item) return this;
      for (let p = item._parent; p; p = p._parent) this._st.open.add(p.id);
      this._paint();
      const row = this._rowEl(item.id);
      if (row) this._scrollTo(row);
      return this;
    }
    select(id) {
      const st = this._st;
      const item = id === null || id === undefined ? null : this.find(id);
      if (item && item.disabled) return this;
      const next = item ? item.id : null;
      st.active = next !== null ? next : st.active;
      if (next === st.selected) return this;
      st.selected = next;
      this._mark();
      emit(this, "selectionchange", { id: next, item: item }, false);
      return this;
    }

    // ---- painting ----------------------------------------------------------------------------------------
    _visible() {
      const st = this._st;
      const out = [];
      const needle = st.filter.trim().toLowerCase();
      // With a filter, keep the matches and their ancestors, and open the ancestors.
      const keep = new Set();
      if (needle) {
        const walk = (items) => {
          let any = false;
          for (const item of items) {
            const own = String(item.label === undefined ? "" : item.label).toLowerCase().includes(needle);
            const below = item.children ? walk(item.children) : false;
            if (own || below) {
              keep.add(item);
              any = true;
            }
          }
          return any;
        };
        walk(st.items);
      }
      const add = (items) => {
        for (const item of items) {
          if (needle && !keep.has(item)) continue;
          out.push(item);
          const open = needle ? !!item.children : st.open.has(item.id);
          if (open && item.children) add(item.children);
        }
      };
      add(st.items);
      return out;
    }
    _paint() {
      const st = this._st;
      if (!st.built) return;
      const h = numberAttr(this, "row-height", 28);
      const indent = numberAttr(this, "indent", 18);
      const rows = this._visible();
      st.rows = rows;
      if (st.active === null || !rows.some((r) => r.id === st.active)) st.active = st.selected !== null && rows.some((r) => r.id === st.selected) ? st.selected : rows.length ? rows[0].id : null;
      this.innerHTML = "";
      if (!rows.length) {
        const empty = document.createElement("div");
        empty.className = "nile-tree-empty";
        empty.textContent = st.filter ? "No matches" : "";
        this.appendChild(empty);
        return;
      }
      const frag = document.createDocumentFragment ? document.createDocumentFragment() : this;
      rows.forEach((item, i) => {
        const row = document.createElement("div");
        row.className = "nile-tree-row";
        row.setAttribute("role", "treeitem");
        row.setAttribute("data-id", String(item.id));
        row.setAttribute("aria-level", String(item._depth + 1));
        row.setAttribute("aria-posinset", String(i + 1));
        row.setAttribute("aria-setsize", String(rows.length));
        row.style.height = h + "px";
        row.style.paddingLeft = 4 + item._depth * indent + "px";
        const kids = this._hasKids(item);
        if (kids) row.setAttribute("aria-expanded", st.open.has(item.id) || (st.filter && item.children) ? "true" : "false");
        if (item.disabled) row.setAttribute("aria-disabled", "true");
        const toggle = document.createElement("span");
        toggle.className = "nile-tree-toggle";
        if (kids) toggle.innerHTML = CHEVRON;
        row.appendChild(toggle);
        if (item.icon) {
          const icon = document.createElement("span");
          icon.className = "nile-tree-icon";
          icon.innerHTML = item.icon;
          row.appendChild(icon);
        }
        const label = document.createElement("span");
        label.className = "nile-tree-label";
        const made = st.render ? st.render(item) : item.label;
        if (made && typeof made === "object" && made.nodeType) label.appendChild(made);
        else label.textContent = made === undefined || made === null ? "" : String(made);
        row.appendChild(label);
        if (st.loading.has(item.id)) {
          const busy = document.createElement("span");
          busy.className = "nile-tree-busy";
          busy.textContent = "…";
          row.appendChild(busy);
        }
        frag.appendChild(row);
      });
      if (frag !== this) this.appendChild(frag);
      this._mark();
    }
    /** Selected and active flags on the rows already drawn. */
    _mark() {
      const st = this._st;
      Array.from(this.children).forEach((row) => {
        const id = row.getAttribute("data-id");
        if (id === null) return;
        row.setAttribute("aria-selected", st.selected !== null && String(st.selected) === id ? "true" : "false");
        if (st.active !== null && String(st.active) === id) row.classList.add("nile-active");
        else row.classList.remove("nile-active");
      });
    }
    _rowEl(id) {
      return Array.from(this.children).find((r) => r.getAttribute("data-id") === String(id)) || null;
    }
    _scrollTo(row) {
      const top = row.offsetTop, bottom = top + row.offsetHeight;
      if (top < this.scrollTop) this.scrollTop = top;
      else if (bottom > this.scrollTop + this.clientHeight) this.scrollTop = bottom - this.clientHeight;
    }
    _itemOf(target) {
      const row = target && target.closest ? target.closest(".nile-tree-row") : null;
      if (!row || row.parentNode !== this) return null;
      return this.find(row.getAttribute("data-id"));
    }

    // ---- input -------------------------------------------------------------------------------------------
    _click(e) {
      const item = this._itemOf(e.target);
      if (!item) return;
      if (e.target.closest && e.target.closest(".nile-tree-toggle")) {
        this._st.active = item.id;
        this.toggle(item.id);
        return;
      }
      this._st.active = item.id;
      this.select(item.id);
    }
    _dblclick(e) {
      const item = this._itemOf(e.target);
      if (!item || item.disabled) return;
      if (this._hasKids(item)) this.toggle(item.id);
      emit(this, "activate", { id: item.id, item: item }, false);
    }
    _onKey(e) {
      const st = this._st;
      const rows = st.rows;
      if (!rows.length || e.ctrlKey || e.metaKey || e.altKey) return;
      let at = rows.findIndex((r) => r.id === st.active);
      if (at < 0) at = 0;
      const item = rows[at];
      let to = null;
      switch (e.key) {
        case "ArrowDown": to = rows[Math.min(rows.length - 1, at + 1)]; break;
        case "ArrowUp": to = rows[Math.max(0, at - 1)]; break;
        case "Home": to = rows[0]; break;
        case "End": to = rows[rows.length - 1]; break;
        case "ArrowRight":
          if (this._hasKids(item) && !st.open.has(item.id)) this.expand(item.id);
          else if (item.children && item.children.length) to = item.children[0];
          break;
        case "ArrowLeft":
          if (st.open.has(item.id) && item.children) this.collapse(item.id);
          else if (item._parent) to = item._parent;
          break;
        case "Enter":
          if (!item.disabled) emit(this, "activate", { id: item.id, item: item }, false);
          break;
        case " ":
          this.select(item.id);
          break;
        case "*":
          (item._parent ? item._parent.children : st.items).forEach((sib) => sib.children && sib.children.length && this.expand(sib.id));
          break;
        default:
          if (e.key.length === 1) {
            // Type-ahead: letters typed in quick succession build a prefix.
            const now = Date.now();
            st.typed = now - st.typedAt > 800 ? e.key.toLowerCase() : st.typed + e.key.toLowerCase();
            st.typedAt = now;
            const start = st.typed.length > 1 ? at : at + 1;
            for (let n = 0; n < rows.length; n++) {
              const r = rows[(start + n) % rows.length];
              if (String(r.label).toLowerCase().startsWith(st.typed)) {
                to = r;
                break;
              }
            }
            if (!to) return;
          } else return;
      }
      e.preventDefault();
      if (to) {
        st.active = to.id;
        if (!to.disabled) this.select(to.id);
        this._mark();
        const row = this._rowEl(to.id);
        if (row) this._scrollTo(row);
      }
    }
  }
  customElements.define("nile-node", NileNode);
  customElements.define("nile-tree", NileTree);
}

defineTree();
