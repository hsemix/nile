// @nile/ui/splitter: <nile-splitter>, resizable panes with a draggable, collapsible grip between them.
//
//   <nile-splitter style="flex: 1">
//     <div size="300" min="180" collapsible>sidebar</div>
//     <div>content</div>
//   </nile-splitter>
//
// Sizes use the same units as <frameset>: a number or `200px`, a percentage (`30%`) or a share of the
// leftover space (`1*`, `2*`). Give them per pane (`size`) or as a list on the host, like a frameset:
// `<nile-splitter cols="200px 1* 30%">` (side by side) or `rows="..."` (stacked).
//
// Host attributes: cols | rows (a list of sizes), orientation (horizontal = side by side, the default; vertical = stacked), grip
// (dots, the default | line | none), persist (a name: sizes and collapsed panes are remembered in
// localStorage). Pane attributes: size (px, %, or N* shares; a pane without one is `1*`),
// min (px, default 40), max (px), collapsible, collapsed. The grip is a focusable separator: drag it,
// arrow keys move it (Shift = bigger steps), a click, Enter or Space collapse/expand the
// neighbouring collapsible pane. Properties: sizes. Methods: setSize(i, px), collapse(i), expand(i),
// toggle(i), isCollapsed(i), refresh(). Event: `splitterchange` (detail: { sizes, collapsed }).

import { style, emit, numberAttr } from "@nile/ui/base";

/** "200", "200px", "30%", "2*" or "*" as { kind: "px" | "pct" | "weight", v }, or null. */
function parseSize(text) {
  const t = String(text === null || text === undefined ? "" : text).trim();
  let m = /^(\d+(?:\.\d+)?)\*$/.exec(t);
  if (m) return { kind: "weight", v: Number(m[1]) };
  if (t === "*") return { kind: "weight", v: 1 };
  m = /^(\d+(?:\.\d+)?)%$/.exec(t);
  if (m) return { kind: "pct", v: Number(m[1]) };
  m = /^(\d+(?:\.\d+)?)(?:px)?$/.exec(t);
  if (m) return { kind: "px", v: Number(m[1]) };
  return null;
}

const CSS = `
:where(nile-splitter) { display: flex; flex-direction: row; min-width: 0; min-height: 0; }
:where(nile-splitter[orientation="vertical"]) { flex-direction: column; }
:where(nile-splitter) > :where(:not(.nile-splitter-grip)) { min-width: 0; min-height: 0; overflow: auto; flex: 1 1 0; box-sizing: border-box; }
:where(nile-splitter) > .nile-collapsed { display: none; }
:where(nile-splitter) > .nile-splitter-grip { flex: 0 0 auto; display: flex; align-items: center; justify-content: center; box-sizing: border-box;
  width: var(--nile-splitter-grip, 10px); background: var(--nile-splitter-bg, rgba(128, 128, 128, 0.14)); color: var(--nile-splitter-color, rgba(128, 128, 128, 0.9));
  cursor: col-resize; user-select: none; outline: none; font: 14px/1 sans-serif; overflow: hidden; }
:where(nile-splitter[orientation="vertical"]) > .nile-splitter-grip { width: auto; height: var(--nile-splitter-grip, 10px); cursor: row-resize; }
:where(nile-splitter) > .nile-splitter-grip:hover, :where(nile-splitter) > .nile-splitter-grip.nile-dragging { background: var(--nile-splitter-hover, rgba(128, 128, 128, 0.32)); }
:where(nile-splitter) > .nile-splitter-grip:focus { background: var(--nile-splitter-hover, rgba(128, 128, 128, 0.32)); }
:where(nile-splitter[grip="none"]) > .nile-splitter-grip { --nile-splitter-grip: 4px; }
:where(nile-splitter[grip="none"]) > .nile-splitter-grip > span, :where(nile-splitter[grip="line"]) > .nile-splitter-grip > span { display: none; }
`;

const px = (n) => Math.round(n * 10) / 10;

export function defineSplitter(tag) {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  const name = tag || "nile-splitter";
  if (customElements.get && customElements.get(name)) return;
  style("splitter", CSS);

  class NileSplitter extends HTMLElement {
    static get observedAttributes() {
      return ["orientation", "grip"];
    }
    _specList() {
      const text = this.getAttribute("cols") || this.getAttribute("rows") || "";
      return text.split(/[\s,]+/).filter(Boolean);
    }
    connectedCallback() {
      this._build();
      if (!this._watch && typeof MutationObserver !== "undefined") {
        this._watch = new MutationObserver((records) => {
          if (this._busy) return;
          if (records.some((r) => Array.from(r.addedNodes).concat(Array.from(r.removedNodes)).some((n) => n.nodeType === 1 && !(n.classList && n.classList.contains("nile-splitter-grip"))))) this.refresh();
        });
        this._watch.observe(this, { childList: true });
      }
    }
    disconnectedCallback() {
      if (this._watch) this._watch.disconnect();
      this._watch = null;
    }
    attributeChangedCallback() {
      if (this._grips) this._labels();
    }
    get _vertical() {
      return this.getAttribute("orientation") === "vertical" || (this.getAttribute("orientation") === null && this.hasAttribute("rows") && !this.hasAttribute("cols"));
    }
    get _panes() {
      return Array.from(this.children).filter((c) => !c.classList.contains("nile-splitter-grip"));
    }
    refresh() {
      this._build();
    }
    _build() {
      this._busy = true;
      // `rows="..."` (like a frameset) stacks the panes.
      if (this.hasAttribute("rows") && !this.hasAttribute("cols") && this.getAttribute("orientation") === null) this.setAttribute("orientation", "vertical");
      for (const g of Array.from(this.children)) if (g.classList.contains("nile-splitter-grip")) this.removeChild(g);
      const panes = this._panes;
      this._grips = [];
      panes.forEach((pane, i) => {
        if (pane._ready) return;
        pane._ready = true;
        const spec = parseSize(pane.getAttribute("size") !== null ? pane.getAttribute("size") : this._specList()[i]);
        if (spec && spec.kind === "px") this._fix(pane, spec.v);
        else if (spec && spec.kind === "pct") {
          pane._fixed = true;
          pane.style.flex = "0 0 " + spec.v + "%";
        } else if (spec) pane.style.flex = spec.v + " 1 0";
        if (pane.hasAttribute("collapsed")) pane.classList.add("nile-collapsed");
        const min = numberAttr(pane, "min", 40);
        if (this._vertical) pane.style.minHeight = min + "px";
        else pane.style.minWidth = min + "px";
      });
      for (let i = 0; i < panes.length - 1; i++) {
        const grip = document.createElement("div");
        grip.className = "nile-splitter-grip";
        grip.setAttribute("role", "separator");
        grip.setAttribute("tabindex", "0");
        grip.innerHTML = "<span></span>";
        this.insertBefore(grip, panes[i + 1]);
        this._grips.push(grip);
        this._wire(grip, i);
      }
      this._labels();
      this._restore();
      this._busy = false;
    }
    _labels() {
      const vertical = this._vertical;
      for (const grip of this._grips || []) {
        grip.setAttribute("aria-orientation", vertical ? "horizontal" : "vertical");
        grip.firstChild.textContent = vertical ? "⋯" : "⋮";
      }
    }
    _fix(pane, size) {
      pane._fixed = true;
      pane.style.flex = "0 0 " + px(size) + "px";
    }
    _size(pane) {
      const r = pane.getBoundingClientRect();
      return this._vertical ? r.height : r.width;
    }
    _bounds(pane) {
      return [numberAttr(pane, "min", 40), numberAttr(pane, "max", Infinity)];
    }
    get sizes() {
      return this._panes.map((p) => (p.classList.contains("nile-collapsed") ? 0 : px(this._size(p))));
    }
    isCollapsed(i) {
      const p = this._panes[i];
      return !!p && p.classList.contains("nile-collapsed");
    }
    /** What dragging the grip after pane `i` changes: the pane, the neighbour that gives up space, and their limits. */
    _drag(i) {
      const panes = this._panes;
      const pane = panes[i];
      if (!pane || pane.classList.contains("nile-collapsed")) return null;
      // The neighbour that gives up space: the next live pane, else the previous one.
      const other = panes.slice(i + 1).find((p) => !p.classList.contains("nile-collapsed")) || panes.slice(0, i).reverse().find((p) => !p.classList.contains("nile-collapsed"));
      const [min, max] = this._bounds(pane);
      const size = this._size(pane);
      const ctx = { pane, other, min, max, size, last: size, total: 0, lo: min, hi: max };
      if (other) {
        ctx.total = size + this._size(other);
        const [omin, omax] = this._bounds(other);
        ctx.lo = Math.max(min, ctx.total - omax);
        ctx.hi = Math.min(max, ctx.total - omin);
      }
      return ctx;
    }
    /** Give the pane `size` px (clamped to the limits measured by `_drag`); true when that changed anything. */
    _resizeTo(ctx, size) {
      const next = Math.max(ctx.lo, Math.min(ctx.hi, size));
      if (next === ctx.last) return false;
      ctx.last = next;
      if (ctx.other && ctx.other._fixed) this._fix(ctx.other, ctx.total - next);
      this._fix(ctx.pane, next);
      return true;
    }
    /** Make pane `i` exactly `size` px (clamped), taking or giving the space to the pane beside it. */
    setSize(i, size) {
      const ctx = this._drag(i);
      if (ctx && this._resizeTo(ctx, size)) this._changed();
    }
    _target(i) {
      // The pane a grip between i and i+1 collapses: the one before it if it can, else the one after.
      const panes = this._panes;
      const before = panes[i], after = panes[i + 1];
      if (before && before.classList.contains("nile-collapsed")) return i;
      if (after && after.classList.contains("nile-collapsed")) return i + 1;
      if (before && before.hasAttribute("collapsible")) return i;
      if (after && after.hasAttribute("collapsible")) return i + 1;
      return -1;
    }
    collapse(i) {
      const pane = this._panes[i];
      if (!pane || pane.classList.contains("nile-collapsed")) return this;
      pane._was = this._size(pane);
      pane.classList.add("nile-collapsed");
      pane.setAttribute("collapsed", "");
      this._changed();
      return this;
    }
    expand(i) {
      const pane = this._panes[i];
      if (!pane || !pane.classList.contains("nile-collapsed")) return this;
      pane.classList.remove("nile-collapsed");
      pane.removeAttribute("collapsed");
      if (pane._fixed && pane._was) this._fix(pane, pane._was);
      this._changed();
      return this;
    }
    toggle(i) {
      return this.isCollapsed(i) ? this.expand(i) : this.collapse(i);
    }
    _wire(grip, i) {
      grip.addEventListener("mousedown", (e) => {
        if (e.button !== 0 && e.button !== undefined) return;
        const ctx = this._drag(i);
        if (!ctx) return;
        const start = this._vertical ? e.clientY : e.clientX;
        this._moved = false;
        grip.classList.add("nile-dragging");
        const move = (m) => {
          const delta = (this._vertical ? m.clientY : m.clientX) - start;
          if (Math.abs(delta) > 3) this._moved = true;
          // Measured once at the press: a move is arithmetic and at most two style writes.
          if (this._moved && this._resizeTo(ctx, ctx.size + delta)) this._soon();
        };
        const up = () => {
          document.removeEventListener("mousemove", move);
          document.removeEventListener("mouseup", up);
          grip.classList.remove("nile-dragging");
          if (this._pending) this._changed();
        };
        document.addEventListener("mousemove", move);
        document.addEventListener("mouseup", up);
        e.preventDefault();
      });
      grip.addEventListener("click", () => {
        // A plain click (not the end of a drag) collapses or expands the neighbouring collapsible pane.
        if (this._moved) {
          this._moved = false;
          return;
        }
        const t = this._target(i);
        if (t >= 0) this.toggle(t);
      });
      grip.addEventListener("keydown", (e) => {
        const step = e.shiftKey ? 50 : 10;
        const less = this._vertical ? "ArrowUp" : "ArrowLeft";
        const more = this._vertical ? "ArrowDown" : "ArrowRight";
        const pane = this._panes[i];
        if (e.key === less || e.key === more) {
          if (pane && !pane.classList.contains("nile-collapsed")) this.setSize(i, this._size(pane) + (e.key === more ? step : -step));
          e.preventDefault();
        } else if (e.key === "Enter" || e.key === " ") {
          const t = this._target(i);
          if (t >= 0) this.toggle(t);
          e.preventDefault();
        }
      });
    }
    /** During a drag: announce the new sizes once per frame instead of on every pointer move. */
    _soon() {
      if (this._pending) return;
      this._pending = true;
      requestAnimationFrame(() => {
        if (this._pending) this._changed();
      });
    }
    _changed() {
      this._pending = false;
      emit(this, "splitterchange", { sizes: this.sizes, collapsed: this._panes.map((p) => p.classList.contains("nile-collapsed")) }, false);
      const key = this.getAttribute("persist");
      if (key) {
        clearTimeout(this._save);
        this._save = setTimeout(() => {
          try {
            localStorage.setItem("nile-splitter:" + key, JSON.stringify({ sizes: this._panes.map((p) => (p._fixed && !p.classList.contains("nile-collapsed") ? px(this._size(p)) : p._fixed ? px(p._was || 0) : null)), collapsed: this._panes.map((p) => p.classList.contains("nile-collapsed")) }));
          } catch (e) {}
        }, 100);
      }
    }
    _restore() {
      const key = this.getAttribute("persist");
      if (!key) return;
      try {
        const saved = JSON.parse(localStorage.getItem("nile-splitter:" + key) || "null");
        if (!saved) return;
        this._panes.forEach((p, i) => {
          if (saved.sizes && typeof saved.sizes[i] === "number" && saved.sizes[i] > 0) this._fix(p, saved.sizes[i]);
          if (saved.collapsed && saved.collapsed[i]) {
            p._was = saved.sizes && saved.sizes[i];
            p.classList.add("nile-collapsed");
            p.setAttribute("collapsed", "");
          }
        });
      } catch (e) {}
    }
  }
  customElements.define(name, NileSplitter);
}

defineSplitter();
