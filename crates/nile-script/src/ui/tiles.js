// @nile/ui/tiles: <nile-tiles>, a grid of large touch-friendly buttons (a POS F-key pad, a launcher, a module menu).
//
//   <nile-tiles columns="4" gap="6">
//     <nile-tile label="Search" shortcut="F3" icon="<svg ...>"></nile-tile>
//     <nile-tile label="Payment" shortcut="F10" variant="success" span="2"></nile-tile>
//     <nile-tile label="Void" variant="danger" badge="3"></nile-tile>
//   </nile-tiles>
//
//   pad.items = [{ label: "Search", shortcut: "F3", icon: SVG, command: "search" }, ...];   // or from data
//   pad.on("tile", (e) => run(e.detail.command, e.detail.item));
//
// <nile-tiles>: `columns` (default 4), `gap` (px, default 6), `row-height` (px; default: tiles size themselves,
// min 72), `items` (array property). <nile-tile>: `label`, `icon` (inline SVG/HTML or an image URL),
// `shortcut` (shown in the corner and pressed for you, e.g. "F3" or "Mod+S"), `variant` (default, primary, success,
// danger, warning), `span` (columns), `row-span`, `badge`, `disabled`, `command`. A tile is a real focusable button:
// Enter/Space press it, arrow keys move between tiles. Event `tile` (detail: { command, item, element }) bubbles
// from the grid; the tile also gets an ordinary `click`. Style with --nile-tile-bg, -color, -border, -hover and the
// variant colours --nile-primary/-success/-danger/-warning, or .nile-tile, .nile-tile-key, .nile-tile-label.

import { style, emit, numberAttr } from "@nile/ui/base";
import { shortcuts } from "@nile/ui/shortcuts";

const CSS = `
:where(nile-tiles) { display: grid; grid-template-columns: repeat(var(--nile-tile-columns, 4), minmax(0, 1fr)); gap: var(--nile-tile-gap, 6px); }
:where(nile-tile) { position: relative; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; box-sizing: border-box; min-height: 72px; padding: 8px; text-align: center; cursor: pointer;
  background: var(--nile-tile-bg, #3a3a3a); color: var(--nile-tile-color, #f2f2f2); border: 1px solid var(--nile-tile-border, rgba(255, 255, 255, 0.14)); border-radius: var(--nile-tile-radius, 3px); outline: none; }
:where(nile-tile:hover) { background: var(--nile-tile-hover, #474747); }
:where(nile-tile:focus-visible) { box-shadow: 0 0 0 2px var(--nile-tile-focus, #38bdf8); }
:where(nile-tile:active) { filter: brightness(0.88); }
:where(nile-tile[variant="primary"]) { background: var(--nile-primary, #2563eb); border-color: transparent; }
:where(nile-tile[variant="success"]) { background: var(--nile-success, #3f9142); border-color: transparent; }
:where(nile-tile[variant="danger"]) { background: var(--nile-danger, #c62828); border-color: transparent; }
:where(nile-tile[variant="warning"]) { background: var(--nile-warning, #d98e04); border-color: transparent; }
:where(nile-tile[variant]:hover) { filter: brightness(1.1); }
:where(nile-tile[disabled]) { opacity: 0.4; cursor: default; pointer-events: none; }
:where(.nile-tile-icon) { display: inline-flex; width: 30px; height: 30px; }
:where(.nile-tile-icon) > svg, :where(.nile-tile-icon) > img { width: 100%; height: 100%; }
:where(.nile-tile-label) { font-size: 14px; line-height: 1.2; }
:where(.nile-tile-key) { position: absolute; left: 7px; top: 5px; font-size: 11px; opacity: 0.7; }
:where(.nile-tile-badge) { position: absolute; right: 6px; top: 5px; min-width: 18px; padding: 0 5px; border-radius: 9px; background: #ef4444; color: #fff; font-size: 11px; line-height: 18px; }
`;

const IMAGE = /\.(png|jpe?g|gif|svg|webp)(\?.*)?$/i;

export function defineTiles() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-tiles")) return;
  style("tiles", CSS);

  class NileTile extends HTMLElement {
    static get observedAttributes() {
      return ["label", "icon", "shortcut", "badge", "span", "row-span", "disabled"];
    }
    connectedCallback() {
      this.setAttribute("role", "button");
      this.setAttribute("tabindex", this.hasAttribute("disabled") ? "-1" : "0");
      if (!this._made) {
        this._made = true;
        this.addEventListener("click", () => this._press());
        this.addEventListener("keydown", (e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            this._press();
          } else if (e.key.indexOf("Arrow") === 0) this._move(e);
        });
      }
      this._build();
    }
    disconnectedCallback() {
      if (this._off) this._off();
      this._off = null;
    }
    attributeChangedCallback() {
      if (this._made) this._build();
    }
    _build() {
      const keep = this._extra || (this._extra = Array.from(this.childNodes).filter((n) => !(n.nodeType === 1 && n.className && String(n.className).indexOf("nile-tile-") === 0)));
      Array.from(this.childNodes).forEach((n) => this.removeChild(n));
      const icon = this.getAttribute("icon");
      if (icon) {
        const span = document.createElement("span");
        span.className = "nile-tile-icon";
        if (IMAGE.test(icon) && icon.indexOf("<") < 0) {
          const img = document.createElement("img");
          img.setAttribute("src", icon);
          span.appendChild(img);
        } else span.innerHTML = icon;
        this.appendChild(span);
      }
      const label = this.getAttribute("label");
      if (label) {
        const l = document.createElement("span");
        l.className = "nile-tile-label";
        l.textContent = label;
        this.appendChild(l);
      }
      keep.forEach((n) => this.appendChild(n));
      const key = this.getAttribute("shortcut");
      if (key) {
        const k = document.createElement("span");
        k.className = "nile-tile-key";
        k.textContent = shortcuts.format(key.split(",")[0]);
        this.appendChild(k);
      }
      const badge = this.getAttribute("badge");
      if (badge) {
        const b = document.createElement("span");
        b.className = "nile-tile-badge";
        b.textContent = badge;
        this.appendChild(b);
      }
      this.style.gridColumn = numberAttr(this, "span", 1) > 1 ? "span " + numberAttr(this, "span", 1) : "";
      this.style.gridRow = numberAttr(this, "row-span", 1) > 1 ? "span " + numberAttr(this, "row-span", 1) : "";
      if (this._off) this._off();
      this._off = null;
      if (key) this._off = shortcuts.on(key, () => this._press(), { label: label || "", when: () => this.isConnected !== false && !this.hasAttribute("disabled") });
    }
    _press() {
      if (this.hasAttribute("disabled")) return;
      const host = this.closest ? this.closest("nile-tiles") : null;
      const item = this._item || null;
      emit(host || this, "tile", { command: this.getAttribute("command") || (item && item.command) || this.getAttribute("label"), item: item, element: this }, false);
      if (item && item.onClick) item.onClick(item);
    }
    _move(e) {
      const host = this.closest("nile-tiles");
      if (!host) return;
      const tiles = Array.from(host.children).filter((n) => n.tagName === "NILE-TILE" && !n.hasAttribute("disabled"));
      const at = tiles.indexOf(this);
      const cols = host._columns();
      const to = { ArrowRight: at + 1, ArrowLeft: at - 1, ArrowDown: at + cols, ArrowUp: at - cols }[e.key];
      if (to >= 0 && to < tiles.length) {
        e.preventDefault();
        tiles[to].focus();
      }
    }
  }

  class NileTiles extends HTMLElement {
    static get observedAttributes() {
      return ["columns", "gap", "row-height"];
    }
    connectedCallback() {
      this._layout();
    }
    attributeChangedCallback() {
      this._layout();
    }
    _columns() {
      return numberAttr(this, "columns", 4);
    }
    _layout() {
      this.style.gridTemplateColumns = "repeat(" + this._columns() + ", minmax(0, 1fr))";
      this.style.gap = numberAttr(this, "gap", 6) + "px";
      const h = numberAttr(this, "row-height", 0);
      this.style.gridAutoRows = h > 0 ? h + "px" : "";
    }
    get items() {
      return this._items || [];
    }
    set items(list) {
      this._items = list || [];
      Array.from(this.children).forEach((n) => {
        if (n.tagName === "NILE-TILE") this.removeChild(n);
      });
      this._items.forEach((item) => {
        const t = document.createElement("nile-tile");
        const set = (name, v) => {
          if (v !== undefined && v !== null && v !== false) t.setAttribute(name, v === true ? "" : String(v));
        };
        set("label", item.label);
        set("icon", item.icon);
        set("shortcut", item.shortcut);
        set("variant", item.variant);
        set("span", item.span);
        set("row-span", item.rowSpan);
        set("badge", item.badge);
        set("disabled", item.disabled);
        set("command", item.command);
        t._item = item;
        this.appendChild(t);
      });
    }
  }
  customElements.define("nile-tile", NileTile);
  customElements.define("nile-tiles", NileTiles);
}

defineTiles();
