// @nile/ui/tabs: <nile-tabs> with <nile-tab> panels.
//
//   <nile-tabs>
//     <nile-tab label="General">...</nile-tab>
//     <nile-tab label="Prices" badge="3">...</nile-tab>
//     <nile-tab label="History" disabled>...</nile-tab>
//   </nile-tabs>
//
// On <nile-tabs>: `selected` (an index, or the `name` of a tab), `position` (top, bottom, left, right), `align`
// (start, center, end, stretch) for the tab strip. On <nile-tab>: `label`, `name`, `badge`, `icon` (HTML, e.g. an
// inline <svg>), `disabled`. Methods: select(indexOrName), refresh() (after adding or removing tabs); properties:
// selectedIndex, selectedTab, tabs. Event: `tabchange` (detail: { index, tab, name }), cancelable `beforetabchange`.
// Keyboard: Left/Right (Up/Down for side strips), Home and End move between tabs, as in a desktop tab control.
// Style with --nile-tabs-accent, --nile-tabs-border, --nile-tabs-bg, or the .nile-tab-strip, .nile-tab-button
// ([aria-selected=true]) and .nile-tab-panels classes.

import { style, emit } from "@nile/ui/base";

const CSS = `
:where(nile-tabs) { display: flex; flex-direction: column; min-height: 0; }
:where(nile-tabs[position="bottom"]) { flex-direction: column-reverse; }
:where(nile-tabs[position="left"]) { flex-direction: row; }
:where(nile-tabs[position="right"]) { flex-direction: row-reverse; }
:where(nile-tabs) > .nile-tab-strip { display: flex; flex: none; border-bottom: 1px solid var(--nile-tabs-border, #cbd5e1); background: var(--nile-tabs-bg, transparent); }
:where(nile-tabs[position="bottom"]) > .nile-tab-strip { border-bottom: 0; border-top: 1px solid var(--nile-tabs-border, #cbd5e1); }
:where(nile-tabs[position="left"]) > .nile-tab-strip, :where(nile-tabs[position="right"]) > .nile-tab-strip { flex-direction: column; border-bottom: 0; border-right: 1px solid var(--nile-tabs-border, #cbd5e1); }
:where(nile-tabs[position="right"]) > .nile-tab-strip { border-right: 0; border-left: 1px solid var(--nile-tabs-border, #cbd5e1); }
:where(nile-tabs[align="center"]) > .nile-tab-strip { justify-content: center; }
:where(nile-tabs[align="end"]) > .nile-tab-strip { justify-content: flex-end; }
:where(nile-tabs[align="stretch"]) > .nile-tab-strip > .nile-tab-button { flex: 1; }
:where(.nile-tab-button) { display: flex; align-items: center; gap: 6px; padding: 9px 16px; border: 0; border-bottom: 2px solid transparent; background: transparent; color: inherit; font: inherit; cursor: pointer; white-space: nowrap; border-radius: 0; box-shadow: none; opacity: 0.75; }
:where(.nile-tab-button:hover) { opacity: 1; background: rgba(127, 127, 127, 0.1); }
:where(.nile-tab-button[aria-selected="true"]) { opacity: 1; border-bottom-color: var(--nile-tabs-accent, #2563eb); font-weight: 600; }
:where(nile-tabs[position="bottom"]) .nile-tab-button { border-bottom: 0; border-top: 2px solid transparent; }
:where(nile-tabs[position="bottom"]) .nile-tab-button[aria-selected="true"] { border-top-color: var(--nile-tabs-accent, #2563eb); }
:where(nile-tabs[position="left"]) .nile-tab-button, :where(nile-tabs[position="right"]) .nile-tab-button { border-bottom: 0; border-left: 2px solid transparent; justify-content: flex-start; }
:where(nile-tabs[position="left"]) .nile-tab-button[aria-selected="true"], :where(nile-tabs[position="right"]) .nile-tab-button[aria-selected="true"] { border-left-color: var(--nile-tabs-accent, #2563eb); }
:where(.nile-tab-button[disabled]) { opacity: 0.35; cursor: default; }
:where(.nile-tab-badge) { min-width: 16px; padding: 0 5px; border-radius: 8px; background: var(--nile-tabs-accent, #2563eb); color: #fff; font-size: 11px; text-align: center; line-height: 16px; }
:where(.nile-tab-icon) { display: inline-flex; }
:where(nile-tabs) > .nile-tab-panels { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
:where(nile-tab) { display: none; flex: 1; min-height: 0; overflow: auto; }
:where(nile-tab.nile-active) { display: block; }
`;

export function defineTabs() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-tabs")) return;
  style("tabs", CSS);

  class NileTab extends HTMLElement {
    static get observedAttributes() {
      return ["label", "badge", "disabled", "icon"];
    }
    connectedCallback() {
      this.setAttribute("role", "tabpanel");
    }
    attributeChangedCallback() {
      const host = this.closest ? this.closest("nile-tabs") : null;
      if (host && host._strip) host.refresh();
    }
  }

  class NileTabs extends HTMLElement {
    static get observedAttributes() {
      return ["selected"];
    }
    connectedCallback() {
      if (!this._strip) {
        this._strip = document.createElement("div");
        this._strip.className = "nile-tab-strip";
        this._strip.setAttribute("role", "tablist");
        this._panels = document.createElement("div");
        this._panels.className = "nile-tab-panels";
        Array.from(this.childNodes).forEach((n) => {
          if (n.nodeType === 1 && n.tagName === "NILE-TAB") this._panels.appendChild(n);
        });
        this.insertBefore(this._strip, this.firstChild);
        this.appendChild(this._panels);
        this._strip.addEventListener("click", (e) => {
          const b = e.target && e.target.closest ? e.target.closest(".nile-tab-button") : null;
          if (b && !b.disabled) this.select(Number(b.getAttribute("data-index")));
        });
        this._strip.addEventListener("keydown", (e) => this._onKey(e));
      }
      this.refresh();
    }
    attributeChangedCallback(attr, was, now) {
      if (this._strip && was !== now) this._show(this._indexOf(now), false);
    }
    get tabs() {
      return this._panels ? Array.from(this._panels.children).filter((n) => n.tagName === "NILE-TAB") : [];
    }
    get selectedIndex() {
      return this.tabs.findIndex((t) => t.classList.contains("nile-active"));
    }
    get selectedTab() {
      return this.tabs[this.selectedIndex] || null;
    }
    _indexOf(v) {
      const tabs = this.tabs;
      if (v === null || v === undefined || v === "") return -1;
      if (typeof v === "number") return v;
      const byName = tabs.findIndex((t) => t.getAttribute("name") === v);
      return byName >= 0 ? byName : /^\d+$/.test(String(v)) ? Number(v) : -1;
    }
    /** Rebuild the strip from the current tabs (after adding, removing or relabelling one). */
    refresh() {
      if (!this._strip) return;
      // Tabs added as direct children after the first build join the panels.
      Array.from(this.childNodes).forEach((n) => {
        if (n.nodeType === 1 && n.tagName === "NILE-TAB") this._panels.appendChild(n);
      });
      const tabs = this.tabs;
      const current = this.selectedIndex >= 0 ? this.selectedIndex : this._indexOf(this.getAttribute("selected"));
      this._strip.innerHTML = "";
      tabs.forEach((tab, i) => {
        const b = document.createElement("button");
        b.className = "nile-tab-button";
        b.setAttribute("role", "tab");
        b.setAttribute("data-index", String(i));
        b.setAttribute("type", "button");
        if (tab.hasAttribute("disabled")) b.disabled = true;
        const icon = tab.getAttribute("icon");
        if (icon) {
          const span = document.createElement("span");
          span.className = "nile-tab-icon";
          span.innerHTML = icon;
          b.appendChild(span);
        }
        const label = document.createElement("span");
        label.textContent = tab.getAttribute("label") || "";
        b.appendChild(label);
        const badge = tab.getAttribute("badge");
        if (badge) {
          const count = document.createElement("span");
          count.className = "nile-tab-badge";
          count.textContent = badge;
          b.appendChild(count);
        }
        this._strip.appendChild(b);
      });
      const pick = current >= 0 && current < tabs.length && !tabs[current].hasAttribute("disabled") ? current : tabs.findIndex((t) => !t.hasAttribute("disabled"));
      this._show(pick, false);
    }
    /** Select a tab by index or `name`. A `beforetabchange` listener can veto it. */
    select(which) {
      const i = this._indexOf(which);
      const tabs = this.tabs;
      if (i < 0 || i >= tabs.length || tabs[i].hasAttribute("disabled") || i === this.selectedIndex) return this;
      if (!emit(this, "beforetabchange", { index: i, tab: tabs[i], name: tabs[i].getAttribute("name") }, true)) return this;
      this._show(i, true);
      return this;
    }
    _show(i, notify) {
      const tabs = this.tabs;
      const buttons = Array.from(this._strip.children);
      tabs.forEach((t, n) => {
        if (n === i) t.classList.add("nile-active");
        else t.classList.remove("nile-active");
        if (buttons[n]) {
          buttons[n].setAttribute("aria-selected", n === i ? "true" : "false");
          buttons[n].setAttribute("tabindex", n === i ? "0" : "-1");
        }
      });
      if (notify && i >= 0) emit(this, "tabchange", { index: i, tab: tabs[i], name: tabs[i].getAttribute("name") }, false);
    }
    _onKey(e) {
      const side = this.getAttribute("position") === "left" || this.getAttribute("position") === "right";
      const next = side ? "ArrowDown" : "ArrowRight";
      const prev = side ? "ArrowUp" : "ArrowLeft";
      const tabs = this.tabs;
      const enabled = tabs.map((t, i) => i).filter((i) => !tabs[i].hasAttribute("disabled"));
      if (!enabled.length) return;
      const at = enabled.indexOf(this.selectedIndex);
      let to = -1;
      if (e.key === next) to = enabled[(at + 1) % enabled.length];
      else if (e.key === prev) to = enabled[(at - 1 + enabled.length) % enabled.length];
      else if (e.key === "Home") to = enabled[0];
      else if (e.key === "End") to = enabled[enabled.length - 1];
      if (to < 0) return;
      e.preventDefault();
      this.select(to);
      const button = this._strip.children[to];
      if (button && button.focus) button.focus();
    }
  }
  customElements.define("nile-tab", NileTab);
  customElements.define("nile-tabs", NileTabs);
}

defineTabs();
