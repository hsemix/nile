// @nile/ui/ribbon: a Microsoft-style ribbon (tabs, groups, large and small buttons, toggles, drop-downs).
// You describe it as data, call ribbon(config), and put the element where you want it:
//
//   import { ribbon } from "@nile/ui";
//   const r = ribbon({
//     file: { label: "File", onClick: () => openBackstage() },          // the coloured first tab (optional)
//     quick: [{ icon: SAVE_SVG, label: "Save", command: "save", shortcut: "Mod+S" }],  // quick access bar (optional)
//     tabs: [
//       { label: "Home", groups: [
//         { label: "Clipboard", items: [
//           { label: "Paste", size: "large", icon: PASTE_SVG, command: "paste",
//             menu: [{ label: "Paste special", command: "pasteSpecial" }, { separator: true }, { label: "Paste as text", command: "pasteText" }] },
//           { type: "stack", items: [{ label: "Cut", icon: CUT_SVG, command: "cut" }, { label: "Copy", icon: COPY_SVG, command: "copy" }] },
//         ] },
//         { label: "Font", items: [
//           { type: "stack", items: [
//             { type: "select", command: "font", options: ["Segoe UI", "Arial"], value: "Segoe UI" },
//             { type: "row", items: [{ type: "toggle", label: "B", command: "bold", title: "Bold", shortcut: "Mod+B" }, { type: "toggle", label: "I", command: "italic" }] } ] } ] },
//       ] },
//       { label: "Table tools", contextual: true, accent: "#7e3fb4", hidden: true, groups: [...] },
//     ],
//   });
//   document.body.prepend(r);
//   r.on("command", (e) => run(e.detail.command, e.detail));          // or give { onCommand } / an item's onClick
//   r.setState("bold", { checked: true }); r.setState("paste", { disabled: true });
//   r.showTab("Table tools"); r.select("Home"); r.collapse(); r.expand();
//
// Items (all optional but `label` or `icon`): type "button" (default), "toggle" (checked), "select" (options, value),
// "stack" (small items in a column), "row" (small items side by side), "separator", "custom" (element: a DOM node).
// Fields: label, icon (inline SVG/HTML, or an image URL ending in .png/.svg/.jpg), size "large" | "small" (default
// small; buttons placed straight in a group default to large), command, menu [{ label, command, icon, checked,
// disabled, shortcut, separator }], split (true: the arrow and the button act separately), disabled, checked, title,
// shortcut (registered with @nile/ui shortcuts and shown in the tooltip), onClick(detail), hidden, html (the label is
// HTML, e.g. "<b>B</b>"), iconOnly (small buttons: hide the label), className (added to the button).
// Events: `command` (detail: { command, item, checked, value, source }), `tabchange`. Methods: select(tab),
// showTab(tab), hideTab(tab), collapse(), expand(), toggleCollapsed(), setState(command, patch), config (get/set,
// re-renders). A collapsed ribbon shows only the tab row; clicking a tab pops its panel open until you pick a command.
// Double-click a tab, or press Ctrl+F1, to collapse/expand. Style freely: variables --nile-ribbon-bg, -tab-bg, -accent,
// -text, -border, -hover, -pressed, -group-label, -file-bg, and every part has a class (.nile-ribbon-tab,
// .nile-ribbon-panel, .nile-ribbon-group, .nile-ribbon-button.nile-large, .nile-ribbon-menu ...). theme="dark" on the
// element switches to a dark palette.

import { style, emit, CHEVRON } from "@nile/ui/base";
import { shortcuts } from "@nile/ui/shortcuts";

const CSS = `
:where(nile-ribbon) { display: block; position: relative; user-select: none; font-size: 12px;
  --rb-bg: var(--nile-ribbon-bg, #f3f3f3); --rb-tab: var(--nile-ribbon-tab-bg, #fafafa); --rb-accent: var(--nile-ribbon-accent, #2b579a); --rb-text: var(--nile-ribbon-text, #262626);
  --rb-border: var(--nile-ribbon-border, #d1d1d1); --rb-hover: var(--nile-ribbon-hover, rgba(0, 0, 0, 0.07)); --rb-pressed: var(--nile-ribbon-pressed, rgba(0, 0, 0, 0.14));
  --rb-label: var(--nile-ribbon-group-label, #606060); --rb-file: var(--nile-ribbon-file-bg, #2b579a);
  background: var(--rb-bg); color: var(--rb-text); border-bottom: 1px solid var(--rb-border); }
:where(nile-ribbon[theme="dark"]) { --rb-bg: var(--nile-ribbon-bg, #2b2b2b); --rb-tab: var(--nile-ribbon-tab-bg, #1f1f1f); --rb-text: var(--nile-ribbon-text, #f0f0f0); --rb-border: var(--nile-ribbon-border, #444);
  --rb-hover: var(--nile-ribbon-hover, rgba(255, 255, 255, 0.1)); --rb-pressed: var(--nile-ribbon-pressed, rgba(255, 255, 255, 0.2)); --rb-label: var(--nile-ribbon-group-label, #a8a8a8); }
:where(.nile-ribbon-top) { display: flex; align-items: center; gap: 2px; padding: 2px 6px; background: var(--rb-tab); }
:where(.nile-ribbon-tabs) { display: flex; align-items: flex-end; gap: 2px; padding: 0 6px; background: var(--rb-tab); }
:where(.nile-ribbon-tab) { padding: 6px 14px 5px; border: 1px solid transparent; border-bottom: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; border-radius: 3px 3px 0 0; box-shadow: none; position: relative; }
:where(.nile-ribbon-tab:hover) { background: var(--rb-hover); }
:where(.nile-ribbon-tab[aria-selected="true"]) { background: var(--rb-bg); border-color: var(--rb-border); margin-bottom: -1px; padding-bottom: 6px; color: var(--rb-accent); }
:where(.nile-ribbon-tab.nile-file) { background: var(--rb-file); color: #fff; margin-right: 6px; border-radius: 0; }
:where(.nile-ribbon-tab.nile-file:hover) { filter: brightness(1.12); }
:where(.nile-ribbon-tab.nile-contextual) { color: var(--rb-ctx, var(--rb-accent)); }
:where(.nile-ribbon-tab.nile-contextual)::before { content: ""; position: absolute; left: 0; right: 0; top: -3px; height: 3px; background: var(--rb-ctx, var(--rb-accent)); }
:where(.nile-ribbon-panel) { display: flex; align-items: stretch; gap: 0; padding: 4px 6px 0; min-height: 84px; background: var(--rb-bg); border-top: 1px solid var(--rb-border); overflow-x: auto; }
:where(nile-ribbon[collapsed]) > .nile-ribbon-panel { display: none; }
:where(nile-ribbon[collapsed]) > .nile-ribbon-panel.nile-pop { display: flex; position: absolute; left: 0; right: 0; z-index: 1250; box-shadow: 0 6px 16px rgba(0, 0, 0, 0.25); }
:where(.nile-ribbon-group) { display: flex; flex-direction: column; padding: 0 6px; border-right: 1px solid var(--rb-border); }
:where(.nile-ribbon-group:last-child) { border-right: 0; }
:where(.nile-ribbon-items) { display: flex; align-items: stretch; gap: 2px; flex: 1; }
:where(.nile-ribbon-group-label) { text-align: center; color: var(--rb-label); font-size: 11px; padding: 2px 4px 3px; white-space: nowrap; }
:where(.nile-ribbon-stack) { display: flex; flex-direction: column; justify-content: space-between; gap: 1px; }
:where(.nile-ribbon-row) { display: flex; gap: 1px; align-items: center; }
:where(.nile-ribbon-sep) { width: 1px; margin: 4px 3px; background: var(--rb-border); align-self: stretch; }
:where(.nile-ribbon-button) { display: flex; align-items: center; gap: 5px; box-sizing: border-box; border: 1px solid transparent; border-radius: 3px; background: transparent; color: inherit; font: inherit; cursor: pointer; padding: 2px 6px; box-shadow: none; white-space: nowrap; }
:where(.nile-ribbon-button:hover) { background: var(--rb-hover); border-color: var(--rb-border); }
:where(.nile-ribbon-button:active), :where(.nile-ribbon-button[aria-pressed="true"]), :where(.nile-ribbon-button[aria-expanded="true"]) { background: var(--rb-pressed); border-color: var(--rb-border); }
:where(.nile-ribbon-button[disabled]) { opacity: 0.4; cursor: default; background: transparent; border-color: transparent; }
:where(.nile-ribbon-button.nile-large) { flex-direction: column; justify-content: flex-start; gap: 2px; min-width: 52px; padding: 4px 6px; text-align: center; }
:where(.nile-ribbon-button) .nile-ribbon-icon { display: inline-flex; width: 16px; height: 16px; flex: none; align-items: center; justify-content: center; }
:where(.nile-ribbon-button.nile-large) .nile-ribbon-icon { width: 32px; height: 32px; }
:where(.nile-ribbon-icon) > svg, :where(.nile-ribbon-icon) > img { width: 100%; height: 100%; }
:where(.nile-ribbon-arrow) { font-size: 9px; opacity: 0.7; }
:where(.nile-ribbon-split) { display: flex; }
:where(.nile-ribbon-split.nile-large) { flex-direction: column; }
:where(.nile-ribbon-select) { font: inherit; padding: 2px 4px; width: 130px; }
:where(.nile-ribbon-menu) { position: fixed; z-index: 1300; min-width: 180px; padding: 3px 0; background: var(--rb-tab); color: var(--rb-text); border: 1px solid var(--rb-border); box-shadow: 0 6px 18px rgba(0, 0, 0, 0.28); font-size: 12px; }
:where(.nile-ribbon-menu-item) { display: flex; align-items: center; gap: 8px; padding: 5px 12px 5px 8px; cursor: pointer; }
:where(.nile-ribbon-menu-item.nile-active) { background: var(--rb-hover); }
:where(.nile-ribbon-menu-item[aria-disabled="true"]) { opacity: 0.4; cursor: default; }
:where(.nile-ribbon-menu-item) .nile-ribbon-icon { width: 16px; height: 16px; display: inline-flex; }
:where(.nile-ribbon-menu-item) .nile-ribbon-check { width: 14px; text-align: center; }
:where(.nile-ribbon-menu-item) .nile-ribbon-key { margin-left: auto; opacity: 0.6; padding-left: 20px; }
:where(.nile-ribbon-menu-sep) { height: 1px; margin: 3px 0; background: var(--rb-border); }
`;

const IMAGE = /\.(png|jpe?g|gif|svg|webp)(\?.*)?$/i;

function iconNode(icon) {
  if (!icon) return null;
  const span = document.createElement("span");
  span.className = "nile-ribbon-icon";
  const text = String(icon);
  if (IMAGE.test(text) && text.indexOf("<") < 0) {
    const img = document.createElement("img");
    img.setAttribute("src", text);
    span.appendChild(img);
  } else span.innerHTML = text;
  return span;
}

function same(a, b) {
  return a !== undefined && b !== undefined && String(a).toLowerCase() === String(b).toLowerCase();
}

export function defineRibbon() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-ribbon")) return;
  style("ribbon", CSS);

  class NileRibbon extends HTMLElement {
    constructor() {
      super();
      this._cfg = { tabs: [] };
      this._at = 0;
      this._offs = [];
    }
    connectedCallback() {
      if (this._wired) return;
      this._wired = true;
      this.addEventListener("keydown", (e) => {
        if (e.ctrlKey && e.key === "F1") {
          e.preventDefault();
          this.toggleCollapsed();
        }
      });
      document.addEventListener("mousedown", (e) => {
        if (this._menu && !this._menu.contains(e.target)) this._closeMenu();
        if (this.hasAttribute("collapsed") && this._panel && this._panel.classList.contains("nile-pop") && !this.contains(e.target)) this._popClose();
      });
      this._render();
    }
    get config() {
      return this._cfg;
    }
    set config(cfg) {
      this._cfg = cfg || { tabs: [] };
      if (typeof this._cfg.selected === "number") this._at = this._cfg.selected;
      else if (typeof this._cfg.selected === "string") this._at = Math.max(0, this._cfg.tabs.findIndex((t) => same(t.label, this._cfg.selected)));
      if (this._cfg.collapsed) this.setAttribute("collapsed", "");
      if (this._cfg.theme) this.setAttribute("theme", this._cfg.theme);
      if (this._wired) this._render();
    }
    // ---- building ---------------------------------------------------------------------------------------------------
    _render() {
      this._offs.forEach((f) => f());
      this._offs = [];
      this._closeMenu();
      this.innerHTML = "";
      this._index = {};
      const cfg = this._cfg;
      if (cfg.quick || cfg.title) {
        const top = document.createElement("div");
        top.className = "nile-ribbon-top";
        (cfg.quick || []).forEach((item) => top.appendChild(this._button(item, "small")));
        if (cfg.title) {
          const t = document.createElement("span");
          t.className = "nile-ribbon-title";
          t.style.margin = "0 auto";
          t.textContent = cfg.title;
          top.appendChild(t);
        }
        this.appendChild(top);
      }
      const strip = document.createElement("div");
      strip.className = "nile-ribbon-tabs";
      strip.setAttribute("role", "tablist");
      if (cfg.file) {
        const f = document.createElement("button");
        f.className = "nile-ribbon-tab nile-file";
        f.setAttribute("type", "button");
        f.textContent = cfg.file.label || "File";
        f.addEventListener("click", () => {
          if (cfg.file.onClick) cfg.file.onClick();
          emit(this, "command", { command: cfg.file.command || "file", item: cfg.file, source: f }, false);
        });
        strip.appendChild(f);
      }
      this._tabEls = [];
      (cfg.tabs || []).forEach((tab, i) => {
        const b = document.createElement("button");
        b.className = "nile-ribbon-tab" + (tab.contextual ? " nile-contextual" : "");
        b.setAttribute("type", "button");
        b.setAttribute("role", "tab");
        b.textContent = tab.label;
        if (tab.accent) b.style.setProperty("--rb-ctx", tab.accent);
        if (tab.hidden) b.style.display = "none";
        b.addEventListener("click", () => this._clicked(i));
        b.addEventListener("dblclick", () => this.toggleCollapsed());
        b.addEventListener("keydown", (e) => this._tabKey(e, i));
        this._tabEls.push(b);
        strip.appendChild(b);
      });
      this.appendChild(strip);
      this._panel = document.createElement("div");
      this._panel.className = "nile-ribbon-panel";
      this._panel.setAttribute("role", "tabpanel");
      this.appendChild(this._panel);
      this._show(this._at);
    }
    _visibleTab(i) {
      const tab = (this._cfg.tabs || [])[i];
      return tab && !tab.hidden;
    }
    _show(i) {
      const tabs = this._cfg.tabs || [];
      if (!this._visibleTab(i)) i = tabs.findIndex((t) => !t.hidden);
      this._at = i < 0 ? 0 : i;
      this._tabEls.forEach((b, n) => {
        b.setAttribute("aria-selected", n === this._at ? "true" : "false");
        b.setAttribute("tabindex", n === this._at ? "0" : "-1");
      });
      this._panel.innerHTML = "";
      const tab = tabs[this._at];
      if (!tab) return;
      (tab.groups || []).forEach((g) => this._panel.appendChild(this._group(g)));
    }
    _group(g) {
      const box = document.createElement("div");
      box.className = "nile-ribbon-group";
      const items = document.createElement("div");
      items.className = "nile-ribbon-items";
      (g.items || []).forEach((item) => {
        const el = this._item(item, "large");
        if (el) items.appendChild(el);
      });
      box.appendChild(items);
      if (g.label) {
        const l = document.createElement("div");
        l.className = "nile-ribbon-group-label";
        l.textContent = g.label;
        box.appendChild(l);
      }
      return box;
    }
    /** Build one item. `fallback` is the size for a plain button placed directly in a group. */
    _item(item, fallback) {
      if (item.hidden) return null;
      const type = item.type || "button";
      if (type === "separator") {
        const s = document.createElement("div");
        s.className = "nile-ribbon-sep";
        return s;
      }
      if (type === "custom") return item.element || null;
      if (type === "stack" || type === "row") {
        const box = document.createElement("div");
        box.className = type === "stack" ? "nile-ribbon-stack" : "nile-ribbon-row";
        (item.items || []).forEach((sub) => {
          const el = this._item(sub, "small");
          if (el) box.appendChild(el);
        });
        return box;
      }
      if (type === "select") return this._select(item);
      return this._button(item, item.size || fallback);
    }
    _register(item, el) {
      if (item.command) (this._index[item.command] = this._index[item.command] || []).push({ item: item, el: el });
    }
    _title(item) {
      const keys = item.shortcut ? " (" + shortcuts.format(String(item.shortcut).split(",")[0]) + ")" : "";
      return (item.title || item.label || "") + keys;
    }
    _button(item, size) {
      const large = size === "large";
      const make = (extraClass) => {
        const b = document.createElement("button");
        b.className = "nile-ribbon-button " + (large ? "nile-large " : "nile-small ") + (extraClass || "");
        b.setAttribute("type", "button");
        const tip = this._title(item);
        if (tip) b.setAttribute("title", tip);
        if (item.command) b.setAttribute("data-command", item.command);
        if (item.className) b.className += " " + item.className;
        if (item.disabled) b.disabled = true;
        return b;
      };
      const body = (b) => {
        const icon = iconNode(item.icon);
        if (icon) b.appendChild(icon);
        if (item.label && (large || !item.iconOnly)) {
          const t = document.createElement("span");
          t.className = "nile-ribbon-label";
          if (item.html) t.innerHTML = item.label;
          else t.textContent = item.label;
          b.appendChild(t);
        }
      };
      const arrow = () => {
        const a = document.createElement("span");
        a.className = "nile-ribbon-arrow";
        a.innerHTML = CHEVRON;
        return a;
      };
      const main = make();
      body(main);
      if (item.toggle || item.type === "toggle") main.setAttribute("aria-pressed", item.checked ? "true" : "false");
      if (item.shortcut) {
        this._offs.push(shortcuts.on(item.shortcut, () => {
          if (!main.disabled) main.click();
        }, { label: item.label }));
      }
      this._register(item, main);
      if (item.menu && item.menu.length && item.split) {
        const wrap = document.createElement("div");
        wrap.className = "nile-ribbon-split" + (large ? " nile-large" : "");
        const drop = make("nile-ribbon-drop");
        drop.appendChild(arrow());
        drop.setAttribute("aria-haspopup", "menu");
        drop.removeAttribute("data-command");
        drop.addEventListener("click", () => this._openMenu(item, drop));
        main.addEventListener("click", () => this._press(item, main));
        wrap.appendChild(main);
        wrap.appendChild(drop);
        return wrap;
      }
      if (item.menu && item.menu.length) {
        main.appendChild(arrow());
        main.setAttribute("aria-haspopup", "menu");
        main.addEventListener("click", () => this._openMenu(item, main));
        return main;
      }
      main.addEventListener("click", () => this._press(item, main));
      return main;
    }
    _select(item) {
      const wrap = document.createElement("label");
      wrap.className = "nile-ribbon-row";
      if (item.label) {
        const t = document.createElement("span");
        t.textContent = item.label;
        wrap.appendChild(t);
      }
      const sel = document.createElement("select");
      sel.className = "nile-ribbon-select";
      (item.options || []).forEach((o) => {
        const opt = document.createElement("option");
        const value = o !== null && typeof o === "object" ? o.value : o;
        opt.value = String(value);
        opt.textContent = o !== null && typeof o === "object" && o.label !== undefined ? o.label : String(value);
        sel.appendChild(opt);
      });
      if (item.value !== undefined) sel.value = String(item.value);
      if (item.disabled) sel.disabled = true;
      sel.addEventListener("change", () => this._fire(item, sel, { value: sel.value }));
      this._register(item, sel);
      wrap.appendChild(sel);
      return wrap;
    }
    // ---- acting -----------------------------------------------------------------------------------------------------
    _press(item, el) {
      if (item.toggle || item.type === "toggle") {
        item.checked = !item.checked;
        el.setAttribute("aria-pressed", item.checked ? "true" : "false");
      }
      this._fire(item, el, {});
      this._popClose();
    }
    _fire(item, source, extra) {
      const detail = Object.assign({ command: item.command, item: item, checked: item.checked, source: source }, extra);
      if (item.onClick) item.onClick(detail);
      emit(this, "command", detail, false);
      if (this._cfg.onCommand) this._cfg.onCommand(detail.command, detail);
    }
    _clicked(i) {
      const was = this._at;
      if (this.hasAttribute("collapsed")) {
        this._show(i);
        this._panel.classList.add("nile-pop");
        this._panel.style.top = this._panel.previousSibling.offsetTop + this._panel.previousSibling.offsetHeight + "px";
      } else this._show(i);
      if (was !== i) emit(this, "tabchange", { index: i, tab: this._cfg.tabs[i], label: this._cfg.tabs[i].label }, false);
    }
    _popClose() {
      if (this._panel && this._panel.classList.contains("nile-pop")) this._panel.classList.remove("nile-pop");
    }
    _tabKey(e, i) {
      const tabs = this._cfg.tabs || [];
      const order = tabs.map((t, n) => n).filter((n) => !tabs[n].hidden);
      const at = order.indexOf(i);
      let to = -1;
      if (e.key === "ArrowRight") to = order[(at + 1) % order.length];
      else if (e.key === "ArrowLeft") to = order[(at - 1 + order.length) % order.length];
      else if (e.key === "Home") to = order[0];
      else if (e.key === "End") to = order[order.length - 1];
      else if (e.key === "ArrowDown" || e.key === "Enter") {
        const first = this._panel.querySelector("button:not([disabled]), select");
        if (first && e.key === "ArrowDown") {
          e.preventDefault();
          first.focus();
        }
        return;
      }
      if (to < 0) return;
      e.preventDefault();
      this._clicked(to);
      this._tabEls[to].focus();
    }
    // ---- menu -------------------------------------------------------------------------------------------------------
    _openMenu(item, anchor) {
      this._closeMenu();
      const menu = document.createElement("div");
      menu.className = "nile-ribbon-menu";
      menu.setAttribute("role", "menu");
      const rows = [];
      item.menu.forEach((m) => {
        if (m.separator) {
          const s = document.createElement("div");
          s.className = "nile-ribbon-menu-sep";
          menu.appendChild(s);
          return;
        }
        const row = document.createElement("div");
        row.className = "nile-ribbon-menu-item";
        row.setAttribute("role", "menuitem");
        if (m.disabled) row.setAttribute("aria-disabled", "true");
        const check = document.createElement("span");
        check.className = "nile-ribbon-check";
        check.textContent = m.checked ? "✓" : "";
        row.appendChild(check);
        const icon = iconNode(m.icon);
        if (icon) row.appendChild(icon);
        const label = document.createElement("span");
        label.textContent = m.label;
        row.appendChild(label);
        if (m.shortcut) {
          const k = document.createElement("span");
          k.className = "nile-ribbon-key";
          k.textContent = shortcuts.format(String(m.shortcut).split(",")[0]);
          row.appendChild(k);
        }
        row.addEventListener("mousedown", (e) => e.stopPropagation());
        row.addEventListener("click", () => {
          if (m.disabled) return;
          this._closeMenu();
          this._fire(m, anchor, { command: m.command, parent: item });
          this._popClose();
        });
        row.addEventListener("mouseenter", () => this._mark(rows.indexOf(row)));
        rows.push(row);
        menu.appendChild(row);
      });
      // Inside the ribbon, so its colour variables (and a dev's overrides) apply; fixed, so nothing clips it.
      this.appendChild(menu);
      const r = anchor.getBoundingClientRect();
      menu.style.left = Math.min(r.left, (window.innerWidth || 800) - menu.offsetWidth - 4) + "px";
      menu.style.top = r.bottom + "px";
      anchor.setAttribute("aria-expanded", "true");
      this._menu = menu;
      this._menuRows = rows;
      this._menuAnchor = anchor;
      this._menuAt = -1;
      this._menuKeys = (e) => this._menuKey(e);
      document.addEventListener("keydown", this._menuKeys, true);
    }
    _mark(at) {
      this._menuAt = at;
      this._menuRows.forEach((r, n) => (n === at ? r.classList.add("nile-active") : r.classList.remove("nile-active")));
    }
    _menuKey(e) {
      const rows = this._menuRows;
      const usable = rows.map((r, n) => n).filter((n) => rows[n].getAttribute("aria-disabled") !== "true");
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        const anchor = this._menuAnchor;
        this._closeMenu();
        if (anchor && anchor.focus) anchor.focus();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const at = usable.indexOf(this._menuAt);
        const next = e.key === "ArrowDown" ? usable[(at + 1) % usable.length] : usable[(at - 1 + usable.length) % usable.length];
        this._mark(next === undefined ? 0 : next);
      } else if (e.key === "Enter" && this._menuAt >= 0) {
        e.preventDefault();
        rows[this._menuAt].click();
      }
    }
    _closeMenu() {
      if (!this._menu) return;
      document.removeEventListener("keydown", this._menuKeys, true);
      if (this._menu.parentNode) this._menu.parentNode.removeChild(this._menu);
      if (this._menuAnchor) this._menuAnchor.setAttribute("aria-expanded", "false");
      this._menu = null;
    }
    // ---- public -----------------------------------------------------------------------------------------------------
    _tabIndex(which) {
      const tabs = this._cfg.tabs || [];
      return typeof which === "number" ? which : tabs.findIndex((t) => same(t.label, which));
    }
    select(which) {
      const i = this._tabIndex(which);
      if (i >= 0 && this._visibleTab(i)) this._clicked(i);
      return this;
    }
    showTab(which, select) {
      const i = this._tabIndex(which);
      if (i < 0) return this;
      this._cfg.tabs[i].hidden = false;
      if (this._tabEls[i]) this._tabEls[i].style.display = "";
      if (select) this.select(i);
      return this;
    }
    hideTab(which) {
      const i = this._tabIndex(which);
      if (i < 0) return this;
      this._cfg.tabs[i].hidden = true;
      if (this._tabEls[i]) this._tabEls[i].style.display = "none";
      if (i === this._at) this._show(0);
      return this;
    }
    collapse() {
      this.setAttribute("collapsed", "");
      this._popClose();
      return this;
    }
    expand() {
      this.removeAttribute("collapsed");
      this._popClose();
      return this;
    }
    toggleCollapsed() {
      return this.hasAttribute("collapsed") ? this.expand() : this.collapse();
    }
    get collapsed() {
      return this.hasAttribute("collapsed");
    }
    get selectedIndex() {
      return this._at;
    }
    /** Change items by command name: { checked, disabled, label, value }. Items on other tabs are updated too. */
    setState(command, patch) {
      const apply = (item) => {
        if (patch.checked !== undefined) item.checked = patch.checked;
        if (patch.disabled !== undefined) item.disabled = patch.disabled;
        if (patch.label !== undefined) item.label = patch.label;
        if (patch.value !== undefined) item.value = patch.value;
      };
      const walk = (items) =>
        (items || []).forEach((item) => {
          if (item.command === command) apply(item);
          walk(item.items);
          walk(item.menu);
        });
      (this._cfg.tabs || []).forEach((t) => (t.groups || []).forEach((g) => walk(g.items)));
      walk(this._cfg.quick);
      (this._index[command] || []).forEach((entry) => {
        const el = entry.el;
        if (patch.disabled !== undefined) el.disabled = !!patch.disabled;
        if (patch.checked !== undefined) el.setAttribute("aria-pressed", patch.checked ? "true" : "false");
        if (patch.value !== undefined && el.tagName === "SELECT") el.value = String(patch.value);
        if (patch.label !== undefined) {
          const t = el.querySelector ? el.querySelector(".nile-ribbon-label") : null;
          if (t) t.textContent = patch.label;
        }
      });
      return this;
    }
  }
  customElements.define("nile-ribbon", NileRibbon);
}

defineRibbon();

/** Create a ribbon from a config object (see the top of this file). Put the returned element in the page. */
export function ribbon(config) {
  defineRibbon();
  const el = document.createElement("nile-ribbon");
  el.config = config;
  return el;
}
