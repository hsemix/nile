// @nile/ui/combobox: <nile-combobox>, a text field with a list of suggestions.
//
//   <nile-combobox id="product" placeholder="Search products" allow-custom>
//     <option value="1">Whole milk</option>
//     <option value="2">Sourdough loaf</option>
//   </nile-combobox>
//
//   product.items = [{ value: 1, label: "Whole milk", hint: "2.50" }, "Plain string", ...];
//   product.source = async (query) => await db.search(query);      // fetched as the user types (debounced)
//   product.on("change", (e) => add(e.detail.item));
//
// Items are strings or { value, label, hint, disabled, group }. Typing filters the list (prefix matches first,
// matching text is wrapped in <mark class="nile-combo-match">); with `source` the list comes from your function
// instead. Attributes: placeholder, value, disabled, allow-custom (the text itself is a valid value),
// open-on-focus ("false" to turn off), limit (rows drawn, default 100), empty (text when nothing matches, default
// "No results"), min-chars (for `source`, default 0), delay (ms before `source` runs, default 150).
// Properties: items, source, value, text, selectedItem, opened, renderItem(item, query) -> string | Node.
// Methods: open(), close(), clear(), focus(). Events: `change` (detail: { value, item }), `input` (detail: { text }),
// `open`, `close`. Keys: Down/Up move, Enter picks, Escape closes and reverts, Tab keeps the highlighted row you
// moved to. Style with --nile-combo-bg, --nile-combo-border, --nile-combo-hover, --nile-combo-selected or the
// .nile-combo-input, .nile-combo-list, .nile-combo-option ([aria-selected]), .nile-combo-hint classes.

import { style, emit, flagAttr, numberAttr, CHEVRON } from "@nile/ui/base";

const CSS = `
:where(nile-combobox) { display: inline-block; position: relative; vertical-align: middle; min-width: 160px; }
:where(nile-combobox) > .nile-combo-box { display: flex; align-items: stretch; border: 1px solid var(--nile-combo-border, #94a3b8); border-radius: 4px; background: var(--nile-combo-bg, Field); color: var(--nile-combo-color, FieldText); }
:where(nile-combobox:focus-within) > .nile-combo-box { border-color: var(--nile-tabs-accent, #2563eb); box-shadow: 0 0 0 1px var(--nile-tabs-accent, #2563eb); }
:where(nile-combobox) .nile-combo-input { flex: 1; min-width: 0; border: 0; outline: none; background: transparent; color: inherit; font: inherit; padding: 6px 8px; box-shadow: none; }
:where(nile-combobox) .nile-combo-toggle { flex: none; width: 26px; border: 0; background: transparent; color: inherit; padding: 0; border-radius: 0; box-shadow: none; cursor: pointer; opacity: 0.7; }
:where(nile-combobox[disabled]) { opacity: 0.55; }
:where(.nile-combo-list) { position: fixed; z-index: 1300; display: none; box-sizing: border-box; max-height: 280px; overflow-y: auto; margin: 0; padding: 4px 0;
  background: var(--nile-combo-list-bg, Canvas); color: var(--nile-combo-color, CanvasText); border: 1px solid var(--nile-combo-border, #94a3b8); border-radius: 4px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25); }
:where(.nile-combo-list.nile-open) { display: block; }
:where(.nile-combo-option) { display: flex; justify-content: space-between; gap: 12px; padding: 6px 10px; cursor: pointer; }
:where(.nile-combo-option.nile-active) { background: var(--nile-combo-hover, rgba(37, 99, 235, 0.14)); }
:where(.nile-combo-option[aria-selected="true"]) { font-weight: 600; color: var(--nile-combo-selected, inherit); }
:where(.nile-combo-option[aria-disabled="true"]) { opacity: 0.4; cursor: default; }
:where(.nile-combo-hint) { opacity: 0.6; flex: none; }
:where(.nile-combo-group) { padding: 6px 10px 2px; font-size: 12px; opacity: 0.6; text-transform: uppercase; letter-spacing: 0.04em; }
:where(.nile-combo-empty) { padding: 8px 10px; opacity: 0.6; }
:where(.nile-combo-match) { background: transparent; color: inherit; font-weight: 700; text-decoration: underline; }
`;

function asItem(x) {
  if (x !== null && typeof x === "object") {
    const label = x.label !== undefined ? String(x.label) : String(x.value);
    return { value: x.value !== undefined ? x.value : label, label: label, hint: x.hint, disabled: !!x.disabled, group: x.group, data: x };
  }
  return { value: x, label: String(x), hint: undefined, disabled: false, group: undefined, data: x };
}

export function defineCombobox() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-combobox")) return;
  style("combobox", CSS);

  class NileCombobox extends HTMLElement {
    static get observedAttributes() {
      return ["value", "placeholder", "disabled"];
    }
    constructor() {
      super();
      this._all = [];
      this._shown = [];
      this._chosen = null;
      this._wanted = null;
      this._at = -1;
      this._moved = false;
      this._opened = false;
      this._seq = 0;
    }
    connectedCallback() {
      if (this._input) return;
      const found = [];
      Array.from(this.childNodes).forEach((n) => {
        if (n.nodeType === 1 && n.tagName === "OPTION") {
          found.push({ value: n.getAttribute("value") !== null ? n.getAttribute("value") : n.textContent, label: n.textContent, disabled: n.hasAttribute("disabled"), hint: n.getAttribute("data-hint") || undefined });
        }
        this.removeChild(n);
      });
      if (found.length && !this._all.length) this._all = found.map(asItem);
      const box = document.createElement("div");
      box.className = "nile-combo-box";
      const input = document.createElement("input");
      input.className = "nile-combo-input";
      input.setAttribute("type", "text");
      input.setAttribute("role", "combobox");
      input.setAttribute("aria-autocomplete", "list");
      input.setAttribute("aria-expanded", "false");
      input.setAttribute("autocomplete", "off");
      const toggle = document.createElement("button");
      toggle.className = "nile-combo-toggle";
      toggle.setAttribute("type", "button");
      toggle.setAttribute("tabindex", "-1");
      toggle.setAttribute("aria-label", "Show options");
      toggle.innerHTML = CHEVRON;
      box.appendChild(input);
      box.appendChild(toggle);
      const list = document.createElement("div");
      list.className = "nile-combo-list";
      list.setAttribute("role", "listbox");
      this.appendChild(box);
      this.appendChild(list);
      this._box = box;
      this._input = input;
      this._list = list;
      input.addEventListener("input", () => this._typed());
      input.addEventListener("keydown", (e) => this._onKey(e));
      input.addEventListener("focus", () => {
        if (flagAttr(this, "open-on-focus", true) && !this._opened && !this.hasAttribute("disabled")) this.open();
      });
      toggle.addEventListener("mousedown", (e) => {
        e.preventDefault();
        if (this.hasAttribute("disabled")) return;
        input.focus();
        if (this._opened) this.close();
        else this.open(true);
      });
      list.addEventListener("mousedown", (e) => {
        e.preventDefault();
        const row = e.target && e.target.closest ? e.target.closest(".nile-combo-option") : null;
        if (row) this._pick(Number(row.getAttribute("data-at")));
      });
      input.addEventListener("blur", () => setTimeout(() => this._left(), 0));
      // An element made through innerHTML is upgraded after its attributes were already set: read `value` now.
      if (this._wanted === null && this.getAttribute("value") !== null) this._wanted = this.getAttribute("value");
      this._syncAttrs();
      this._resolve();
    }
    attributeChangedCallback(attr, was, now) {
      if (was === now) return;
      if (attr === "value") {
        this._wanted = now;
        if (this._input) this._resolve();
      } else if (this._input) this._syncAttrs();
    }
    _syncAttrs() {
      this._input.setAttribute("placeholder", this.getAttribute("placeholder") || "");
      this._input.disabled = this.hasAttribute("disabled");
    }
    // ---- data -------------------------------------------------------------------------------------------------
    get items() {
      return this._all.map((i) => i.data);
    }
    set items(list) {
      this._all = (list || []).map(asItem);
      if (this._input) {
        this._resolve();
        if (this._opened) this._refilter();
      }
    }
    get source() {
      return this._source || null;
    }
    set source(fn) {
      this._source = fn;
    }
    get value() {
      if (this._dirty && this.hasAttribute("allow-custom") && this._input && this._input.value) return this._input.value;
      if (this._chosen) return this._chosen.value;
      if (this.hasAttribute("allow-custom") && this._input && this._input.value) return this._input.value;
      return this._wanted === null ? "" : this._wanted;
    }
    set value(v) {
      this._wanted = v === null || v === undefined ? null : v;
      if (this._input) this._resolve();
      else if (this._wanted !== null) this.setAttribute("value", String(this._wanted));
    }
    get text() {
      return this._input ? this._input.value : "";
    }
    set text(t) {
      if (this._input) this._input.value = t === null || t === undefined ? "" : String(t);
    }
    get selectedItem() {
      return this._chosen ? this._chosen.data : null;
    }
    get opened() {
      return this._opened;
    }
    /** Find the wanted value among the items and show its label. */
    _resolve() {
      if (!this._input) return;
      if (this._wanted === null || this._wanted === "") {
        if (this._wanted === "") this._chosen = null;
        if (this._wanted === "") this._input.value = "";
        return;
      }
      const hit = this._all.find((i) => String(i.value) === String(this._wanted));
      if (hit) {
        this._chosen = hit;
        this._input.value = hit.label;
      } else if (this.hasAttribute("allow-custom")) {
        this._chosen = null;
        this._input.value = String(this._wanted);
      }
    }
    // ---- list ----------------------------------------------------------------------------------------------------
    _match(query) {
      const q = query.trim().toLowerCase();
      if (!q) return this._all.slice();
      const starts = [], inside = [];
      this._all.forEach((i) => {
        const at = i.label.toLowerCase().indexOf(q);
        if (at === 0) starts.push(i);
        else if (at > 0 || (i.hint !== undefined && String(i.hint).toLowerCase().indexOf(q) >= 0)) inside.push(i);
      });
      return starts.concat(inside);
    }
    _typed() {
      // The last choice stays until the text is committed, so Escape or a blur can go back to it.
      this._dirty = true;
      emit(this, "input", { text: this._input.value }, false);
      this._moved = false;
      if (!this._opened) this._show();
      this._refilter();
    }
    _refilter() {
      const query = this._input.value;
      const src = this._source;
      if (!src) {
        this._draw(this._match(query), query);
        return;
      }
      if (query.length < numberAttr(this, "min-chars", 0)) {
        this._draw([], query);
        return;
      }
      const seq = ++this._seq;
      clearTimeout(this._wait);
      this._wait = setTimeout(() => {
        Promise.resolve(src(query)).then((found) => {
          if (seq !== this._seq || !this._opened) return;
          this._all = (found || []).map(asItem);
          this._draw(this._all.slice(), query);
        });
      }, numberAttr(this, "delay", 150));
    }
    _draw(rows, query) {
      const limit = numberAttr(this, "limit", 100);
      this._shown = rows.slice(0, limit);
      this._list.innerHTML = "";
      let group;
      if (!this._shown.length) {
        const none = document.createElement("div");
        none.className = "nile-combo-empty";
        none.textContent = this.getAttribute("empty") || "No results";
        this._list.appendChild(none);
      }
      this._shown.forEach((item, at) => {
        if (item.group !== undefined && item.group !== group) {
          group = item.group;
          const g = document.createElement("div");
          g.className = "nile-combo-group";
          g.textContent = String(group);
          this._list.appendChild(g);
        }
        const row = document.createElement("div");
        row.className = "nile-combo-option";
        row.setAttribute("role", "option");
        row.setAttribute("data-at", String(at));
        row.setAttribute("aria-selected", this._chosen === item ? "true" : "false");
        if (item.disabled) row.setAttribute("aria-disabled", "true");
        const made = this.renderItem ? this.renderItem(item.data, query) : null;
        if (made !== null && made !== undefined) {
          if (made.nodeType) row.appendChild(made);
          else row.innerHTML = String(made);
        } else {
          const label = document.createElement("span");
          this._marked(label, item.label, query);
          row.appendChild(label);
          if (item.hint !== undefined) {
            const hint = document.createElement("span");
            hint.className = "nile-combo-hint";
            hint.textContent = String(item.hint);
            row.appendChild(hint);
          }
        }
        this._list.appendChild(row);
      });
      this._at = -1;
      const keep = this._shown.indexOf(this._chosen);
      if (keep >= 0) this._activate(keep, false);
      else if (query.trim() && this._shown.length) this._activate(this._shown.findIndex((i) => !i.disabled), false);
      this._place();
    }
    _marked(span, text, query) {
      const q = query.trim();
      const at = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
      if (at < 0) {
        span.textContent = text;
        return;
      }
      span.appendChild(document.createTextNode(text.slice(0, at)));
      const mark = document.createElement("mark");
      mark.className = "nile-combo-match";
      mark.textContent = text.slice(at, at + q.length);
      span.appendChild(mark);
      span.appendChild(document.createTextNode(text.slice(at + q.length)));
    }
    _activate(at, fromKeys) {
      const rows = this._list.querySelectorAll(".nile-combo-option");
      rows.forEach((r) => r.classList.remove("nile-active"));
      this._at = at;
      if (at >= 0 && rows[at]) {
        rows[at].classList.add("nile-active");
        this._input.setAttribute("aria-activedescendant", "");
        const row = rows[at];
        const top = row.offsetTop, bottom = top + row.offsetHeight;
        if (top < this._list.scrollTop) this._list.scrollTop = top;
        else if (bottom > this._list.scrollTop + this._list.clientHeight) this._list.scrollTop = bottom - this._list.clientHeight;
      }
      if (fromKeys) this._moved = true;
    }
    // ---- opening ---------------------------------------------------------------------------------------------------
    /** Show the list. With `all` (the chevron), every item shows even when the text matches one. */
    open(all) {
      if (this.hasAttribute("disabled") || !this._input) return this;
      this._show();
      const text = this._input.value;
      if (all || (this._chosen && this._chosen.label === text)) {
        if (this._source) this._refilter();
        else this._draw(this._all.slice(), "");
      } else this._refilter();
      return this;
    }
    _show() {
      if (this._opened) return;
      this._opened = true;
      this._list.classList.add("nile-open");
      this._input.setAttribute("aria-expanded", "true");
      emit(this, "open", null, false);
    }
    _place() {
      if (!this._opened) return;
      const r = this._box.getBoundingClientRect();
      const list = this._list;
      list.style.minWidth = r.width + "px";
      list.style.left = r.left + "px";
      const room = (window.innerHeight || 600) - r.bottom;
      const height = Math.min(list.scrollHeight + 10, 280);
      if (room < height && r.top > room) {
        list.style.top = "";
        list.style.bottom = (window.innerHeight || 600) - r.top + "px";
        list.style.maxHeight = Math.min(280, r.top - 8) + "px";
      } else {
        list.style.bottom = "";
        list.style.top = r.bottom + "px";
        list.style.maxHeight = Math.min(280, Math.max(120, room - 8)) + "px";
      }
    }
    close() {
      if (!this._opened) return this;
      this._opened = false;
      clearTimeout(this._wait);
      this._seq++;
      this._list.classList.remove("nile-open");
      this._input.setAttribute("aria-expanded", "false");
      emit(this, "close", null, false);
      return this;
    }
    clear() {
      this._dirty = false;
      this._chosen = null;
      this._wanted = null;
      if (this._input) this._input.value = "";
      emit(this, "change", { value: "", item: null }, false);
      return this;
    }
    focus() {
      if (this._input) this._input.focus();
    }
    // ---- choosing --------------------------------------------------------------------------------------------------
    _pick(at) {
      const item = this._shown[at];
      if (!item || item.disabled) return;
      const changed = this._chosen !== item;
      this._dirty = false;
      this._chosen = item;
      this._wanted = item.value;
      this._input.value = item.label;
      this.close();
      if (changed) emit(this, "change", { value: item.value, item: item.data }, false);
    }
    _commitText() {
      const text = this._input.value;
      this._dirty = false;
      if (this._chosen && this._chosen.label === text) return;
      const exact = this._all.find((i) => i.label.toLowerCase() === text.trim().toLowerCase() && !i.disabled);
      if (exact) {
        this._chosen = exact;
        this._wanted = exact.value;
        this._input.value = exact.label;
        emit(this, "change", { value: exact.value, item: exact.data }, false);
      } else if (this.hasAttribute("allow-custom") && text) {
        this._chosen = null;
        this._wanted = text;
        emit(this, "change", { value: text, item: null }, false);
      } else {
        // Not a valid entry: go back to the last choice.
        this._input.value = this._chosen ? this._chosen.label : "";
      }
    }
    _left() {
      if (!this._input || this.contains(document.activeElement)) return;
      this._commitText();
      this.close();
    }
    _onKey(e) {
      const k = e.key;
      if (k === "ArrowDown" || k === "ArrowUp") {
        e.preventDefault();
        if (!this._opened) {
          this.open(true);
          return;
        }
        const n = this._shown.length;
        if (!n) return;
        let at = this._at;
        for (let tries = 0; tries < n; tries++) {
          at = k === "ArrowDown" ? (at + 1) % n : (at - 1 + n) % n;
          if (!this._shown[at].disabled) break;
        }
        this._activate(at, true);
      } else if (k === "Home" && this._opened && e.ctrlKey) {
        this._activate(0, true);
      } else if (k === "Enter") {
        if (this._opened && this._at >= 0) {
          e.preventDefault();
          this._pick(this._at);
        } else {
          this._commitText();
          this.close();
        }
      } else if (k === "Escape") {
        if (this._opened) {
          e.preventDefault();
          e.stopPropagation();
          this._input.value = this._chosen ? this._chosen.label : "";
          this.close();
        }
      } else if (k === "Tab") {
        if (this._opened && this._moved && this._at >= 0) this._pick(this._at);
        else {
          this._commitText();
          this.close();
        }
      }
    }
  }
  customElements.define("nile-combobox", NileCombobox);
}

defineCombobox();
