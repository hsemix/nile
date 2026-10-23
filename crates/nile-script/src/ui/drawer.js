// @nile/ui/drawer: <nile-drawer>, a panel that slides in from an edge of the window.
//
//   <button data-drawer-toggle="#admin">...</button>
//   <nile-drawer id="admin" side="right" size="360">
//     <h2>Admin</h2> ... <button data-drawer-close>Close</button>
//   </nile-drawer>
//
// Attributes: side (right|left|top|bottom, default right), size (px or any CSS length; default 320px),
// open (present = shown), backdrop (default on; "false" lets the page behind stay usable), dismissible
// (default on: Escape and a click on the backdrop close it). Properties: opened. Methods: show(), hide(),
// toggle(). Events: `open`, `close`, and a cancelable `beforeclose` (preventDefault keeps it open).
// Triggers without script: data-drawer-toggle / data-drawer-open ("#selector"), data-drawer-close
// (no value = the drawer it sits in).

import { style, emit, focusable, flagAttr, lengthAttr } from "@nile/ui/base";

const CSS = `
:where(nile-drawer) { display: none; position: fixed; inset: 0; z-index: 1000; }
:where(nile-drawer.nile-shown) { display: block; }
:where(nile-drawer[backdrop="false"]) { pointer-events: none; }
:where(nile-drawer) > .nile-drawer-backdrop { position: absolute; inset: 0; background: rgba(0, 0, 0, 0.4); opacity: 0; transition: opacity 0.2s ease; }
:where(nile-drawer[backdrop="false"]) > .nile-drawer-backdrop { display: none; }
:where(nile-drawer.nile-open) > .nile-drawer-backdrop { opacity: 1; }
:where(nile-drawer) > .nile-drawer-panel { position: absolute; box-sizing: border-box; overflow: auto; pointer-events: auto; outline: none;
  background: var(--nile-drawer-bg, Canvas); color: var(--nile-drawer-color, CanvasText);
  box-shadow: var(--nile-drawer-shadow, 0 0 24px rgba(0, 0, 0, 0.35)); transition: transform 0.2s ease; }
:where(nile-drawer:not([side]), nile-drawer[side="right"]) > .nile-drawer-panel { top: 0; bottom: 0; right: 0; max-width: 100%; transform: translateX(100%); }
:where(nile-drawer[side="left"]) > .nile-drawer-panel { top: 0; bottom: 0; left: 0; max-width: 100%; transform: translateX(-100%); }
:where(nile-drawer[side="top"]) > .nile-drawer-panel { left: 0; right: 0; top: 0; max-height: 100%; transform: translateY(-100%); }
:where(nile-drawer[side="bottom"]) > .nile-drawer-panel { left: 0; right: 0; bottom: 0; max-height: 100%; transform: translateY(100%); }
:where(nile-drawer.nile-open) > .nile-drawer-panel { transform: none; }
`;

const SLIDE_MS = 220;
const open = new Set();

function targetOf(el, selector) {
  if (selector) return document.querySelector(selector);
  return el.closest("nile-drawer");
}

export function defineDrawer(tag) {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  const name = tag || "nile-drawer";
  if (customElements.get && customElements.get(name)) return;
  style("drawer", CSS);

  class NileDrawer extends HTMLElement {
    static get observedAttributes() {
      return ["open", "size", "side"];
    }
    connectedCallback() {
      if (!this._panel) this._build();
      this._size();
      if (this.hasAttribute("open") && !this._opened) this._reveal();
    }
    disconnectedCallback() {
      open.delete(this);
    }
    attributeChangedCallback(attr) {
      if (!this._panel) return;
      if (attr === "open") {
        if (this.hasAttribute("open")) this._reveal();
        else this._conceal();
      } else this._size();
    }
    _build() {
      const backdrop = document.createElement("div");
      backdrop.className = "nile-drawer-backdrop";
      const panel = document.createElement("div");
      panel.className = "nile-drawer-panel";
      panel.setAttribute("role", "dialog");
      panel.setAttribute("tabindex", "-1");
      while (this.firstChild) panel.appendChild(this.firstChild);
      this.appendChild(backdrop);
      this.appendChild(panel);
      this._panel = panel;
      this._opened = false;
      backdrop.addEventListener("mousedown", () => {
        if (flagAttr(this, "dismissible", true)) this.hide();
      });
    }
    _size() {
      const vertical = this.getAttribute("side") === "top" || this.getAttribute("side") === "bottom";
      this._panel.style.width = vertical ? "" : lengthAttr(this, "size", "320px");
      this._panel.style.height = vertical ? lengthAttr(this, "size", "320px") : "";
    }
    get opened() {
      return this._opened === true;
    }
    show() {
      this.setAttribute("open", "");
      return this;
    }
    hide() {
      if (!this._opened) return this;
      if (!emit(this, "beforeclose", null, true)) return this;
      this.removeAttribute("open");
      return this;
    }
    toggle() {
      return this._opened ? this.hide() : this.show();
    }
    _reveal() {
      if (this._opened) return;
      this._opened = true;
      this._before = document.activeElement;
      clearTimeout(this._timer);
      this.classList.add("nile-shown");
      this._timer = setTimeout(() => {
        this.classList.add("nile-open");
        const first = this._panel.querySelector("[autofocus]") || focusable(this._panel)[0] || this._panel;
        if (first && first.focus) first.focus();
      }, 20);
      open.add(this);
      emit(this, "open", null, false);
    }
    _conceal() {
      if (!this._opened) return;
      this._opened = false;
      open.delete(this);
      clearTimeout(this._timer);
      this.classList.remove("nile-open");
      this._timer = setTimeout(() => this.classList.remove("nile-shown"), SLIDE_MS);
      const before = this._before;
      this._before = null;
      if (before && before.focus && before.isConnected !== false) before.focus();
      emit(this, "close", null, false);
    }
  }
  customElements.define(name, NileDrawer);
}

let wired = false;
function wire() {
  if (wired || typeof document === "undefined") return;
  wired = true;
  document.addEventListener("click", (e) => {
    const t = e.target && e.target.closest ? e.target : null;
    if (!t) return;
    const toggle = t.closest("[data-drawer-toggle]");
    const show = t.closest("[data-drawer-open]");
    const hide = t.closest("[data-drawer-close]");
    if (toggle) {
      const d = targetOf(toggle, toggle.getAttribute("data-drawer-toggle"));
      if (d && d.toggle) d.toggle();
    } else if (show) {
      const d = targetOf(show, show.getAttribute("data-drawer-open"));
      if (d && d.show) d.show();
    } else if (hide) {
      const d = targetOf(hide, hide.getAttribute("data-drawer-close"));
      if (d && d.hide) d.hide();
    }
  });
  document.addEventListener(
    "keydown",
    (e) => {
      if (!open.size) return;
      const top = Array.from(open).pop();
      if (e.key === "Escape" && flagAttr(top, "dismissible", true)) {
        top.hide();
        e.preventDefault();
      } else if (e.key === "Tab" && flagAttr(top, "backdrop", true)) {
        const items = focusable(top._panel);
        if (!items.length) {
          e.preventDefault();
          return;
        }
        const first = items[0], last = items[items.length - 1];
        const inside = top._panel.contains(document.activeElement);
        if (e.shiftKey && (document.activeElement === first || !inside)) {
          last.focus();
          e.preventDefault();
        } else if (!e.shiftKey && (document.activeElement === last || !inside)) {
          first.focus();
          e.preventDefault();
        }
      }
    },
    true,
  );
}

defineDrawer();
wire();
