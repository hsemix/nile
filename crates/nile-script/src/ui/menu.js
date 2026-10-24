// @nile/ui/menu: popup and context menus from plain data.
//
//   import { menu, contextMenu } from "@nile/ui";
//   const choice = await menu([
//     { label: "Open", shortcut: "Mod+O", run: openIt },
//     { label: "Recent", items: [{ label: "report.xlsx" }, { label: "notes.txt" }] },      // a submenu
//     { separator: true },
//     { label: "Word wrap", checked: true, run: toggleWrap },
//     { label: "Delete", disabled: !canDelete, danger: true },
//   ], { anchor: button });                       // under an element; or { x, y } for a point (e.g. the pointer)
//   contextMenu(listEl, (event) => [ ... ]);      // right-click menu; items may be an array or a function
//
// An item: { label, shortcut ("Mod+S", shown as Ctrl+S or the Mac symbols), icon (HTML such as an inline <svg>),
// checked (true/false adds a check mark), disabled, danger, separator, items (a submenu), run(item), id, data... }.
// `menu()` returns a Promise of the chosen item (its `run` has been called) or null when it was dismissed. Options:
// anchor (element), x / y, align ("start" or "end" for anchored menus), width (min px), event (the contextmenu event,
// for the position), id, restoreFocus (default true). `contextMenu(el, items)` returns a function that removes it.
// Only one menu tree is open at a time. Keyboard: Up/Down, Home/End, Right opens a submenu, Left or Escape closes
// one, Enter/Space chooses; a letter jumps to the next label starting with it; Tab closes. The menu is a
// role="menu" popup with menuitem / menuitemcheckbox rows. Style with --nile-menu-bg, --nile-menu-color,
// --nile-menu-hover, --nile-menu-border, --nile-menu-danger or the .nile-menu-popup, .nile-menu-row (.nile-active,
// [aria-disabled]), .nile-menu-sep classes.

import { style, modal, CHEVRON } from "@nile/ui/base";
import { formatShortcut } from "@nile/ui/shortcuts";

const CSS = `
:where(.nile-menu-popup) { position: fixed; z-index: 1400; box-sizing: border-box; min-width: 160px; max-height: 80%; overflow-y: auto; padding: 4px 0; background: var(--nile-menu-bg, Canvas); color: var(--nile-menu-color, CanvasText); border: 1px solid var(--nile-menu-border, rgba(127, 127, 127, 0.5)); border-radius: 6px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3); outline: none; font-size: 14px; }
:where(.nile-menu-row) { display: flex; align-items: center; gap: 8px; padding: 6px 12px 6px 8px; cursor: default; white-space: nowrap; }
:where(.nile-menu-row.nile-active) { background: var(--nile-menu-hover, rgba(37, 99, 235, 0.2)); }
:where(.nile-menu-row[aria-disabled="true"]) { opacity: 0.4; }
:where(.nile-menu-row.nile-danger) { color: var(--nile-menu-danger, #dc2626); }
:where(.nile-menu-check) { flex: none; width: 16px; text-align: center; font-size: 13px; }
:where(.nile-menu-icon) { flex: none; display: flex; align-items: center; width: 16px; }
:where(.nile-menu-label) { flex: 1; }
:where(.nile-menu-key) { flex: none; margin-left: 24px; opacity: 0.6; font-size: 12px; }
:where(.nile-menu-more) { flex: none; width: 10px; opacity: 0.7; }
:where(.nile-menu-more svg) { width: 9px; height: 6px; transform: rotate(-90deg); }
:where(.nile-menu-sep) { height: 1px; margin: 4px 0; background: var(--nile-menu-border, rgba(127, 127, 127, 0.4)); }
`;

let current = null;

function selectable(item) {
  return item && !item.separator && !item.disabled;
}

/** Open a menu tree; resolves with the chosen item or null. */
export function menu(items, options) {
  const o = options || {};
  style("menu", CSS);
  if (current) current.close(null);
  const list = typeof items === "function" ? items() : items;
  return new Promise((resolve) => {
    const levels = [];
    const opener = document.activeElement;
    let done = false;
    modal.depth++;

    function finish(choice) {
      if (done) return;
      done = true;
      modal.depth = Math.max(0, modal.depth - 1);
      document.removeEventListener("mousedown", outside, true);
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("wheel", wheel, true);
      levels.splice(0).forEach((l) => l.el.remove());
      current = null;
      if (o.restoreFocus !== false && opener && opener.focus && opener !== document.body) opener.focus();
      if (choice && typeof choice.run === "function") choice.run(choice);
      resolve(choice || null);
    }
    function outside(e) {
      if (!levels.some((l) => l.el.contains(e.target))) finish(null);
    }
    function wheel(e) {
      if (!levels.some((l) => l.el.contains(e.target))) finish(null);
    }
    current = { close: finish };

    function closeTo(depth) {
      while (levels.length > depth) levels.pop().el.remove();
    }
    function setActive(level, i) {
      level.active = i;
      Array.from(level.el.children).forEach((row, n) => {
        if (n === i) row.classList.add("nile-active");
        else row.classList.remove("nile-active");
      });
      const row = level.el.children[i];
      if (row) {
        level.el.setAttribute("aria-activedescendant", row.id || "");
        const top = row.offsetTop, bottom = top + row.offsetHeight;
        if (top < level.el.scrollTop) level.el.scrollTop = top;
        else if (bottom > level.el.scrollTop + level.el.clientHeight) level.el.scrollTop = bottom - level.el.clientHeight;
      }
    }
    function move(level, dir, from) {
      const n = level.items.length;
      let i = from;
      for (let k = 0; k < n; k++) {
        i = (i + dir + n) % n;
        if (selectable(level.items[i])) return i;
      }
      return from;
    }
    function choose(level, i) {
      const item = level.items[i];
      if (!selectable(item)) return;
      if (item.items && item.items.length) {
        openLevel(item.items, level, i);
        return;
      }
      finish(item);
    }
    function openLevel(children, parent, parentIndex) {
      closeTo(parent ? levels.indexOf(parent) + 1 : 0);
      const el = document.createElement("div");
      el.className = "nile-menu-popup";
      el.setAttribute("role", "menu");
      el.setAttribute("tabindex", "-1");
      if (o.width) el.style.minWidth = o.width + "px";
      const level = { el: el, items: children, active: -1, parentIndex: parentIndex };
      children.forEach((item, i) => {
        const row = document.createElement("div");
        if (item.separator) {
          row.className = "nile-menu-sep";
          row.setAttribute("role", "separator");
          el.appendChild(row);
          return;
        }
        row.className = "nile-menu-row" + (item.danger ? " nile-danger" : "");
        row.id = "nile-menu-" + levels.length + "-" + i;
        row.setAttribute("role", item.checked !== undefined ? "menuitemcheckbox" : "menuitem");
        if (item.checked !== undefined) row.setAttribute("aria-checked", item.checked ? "true" : "false");
        if (item.disabled) row.setAttribute("aria-disabled", "true");
        if (item.items && item.items.length) row.setAttribute("aria-haspopup", "true");
        const check = document.createElement("span");
        check.className = "nile-menu-check";
        check.textContent = item.checked ? "✓" : "";
        row.appendChild(check);
        if (item.icon) {
          const icon = document.createElement("span");
          icon.className = "nile-menu-icon";
          icon.innerHTML = item.icon;
          row.appendChild(icon);
        }
        const label = document.createElement("span");
        label.className = "nile-menu-label";
        label.textContent = item.label === undefined ? "" : String(item.label);
        row.appendChild(label);
        if (item.shortcut) {
          const key = document.createElement("span");
          key.className = "nile-menu-key";
          key.textContent = formatShortcut(item.shortcut);
          row.appendChild(key);
        }
        if (item.items && item.items.length) {
          const more = document.createElement("span");
          more.className = "nile-menu-more";
          more.innerHTML = CHEVRON;
          row.appendChild(more);
        }
        row.addEventListener("mouseenter", () => {
          if (!selectable(item)) return;
          closeTo(levels.indexOf(level) + 1);
          setActive(level, i);
          if (item.items && item.items.length) openLevel(item.items, level, i);
        });
        row.addEventListener("mousedown", (e) => e.preventDefault());
        row.addEventListener("click", () => choose(level, i));
        el.appendChild(row);
      });
      document.body.appendChild(el);
      levels.push(level);
      position(level, parent);
      if (!parent) el.focus();
      return level;
    }
    function position(level, parent) {
      const el = level.el;
      const w = el.offsetWidth, h = el.offsetHeight;
      const vw = window.innerWidth || 800, vh = window.innerHeight || 600;
      let x, y;
      if (parent) {
        const row = parent.el.children[level.parentIndex];
        const r = parent.el.getBoundingClientRect();
        const rr = row.getBoundingClientRect();
        x = r.right - 2;
        y = rr.top - 4;
        if (x + w > vw) x = r.left - w + 2;
      } else if (o.anchor && o.anchor.getBoundingClientRect) {
        const r = o.anchor.getBoundingClientRect();
        x = o.align === "end" ? r.right - w : r.left;
        y = r.bottom;
        if (y + h > vh && r.top > vh - r.bottom) y = r.top - h;
      } else {
        const ev = o.event;
        x = o.x !== undefined ? o.x : ev ? ev.clientX : 0;
        y = o.y !== undefined ? o.y : ev ? ev.clientY : 0;
      }
      el.style.left = Math.max(0, Math.min(x, vw - w)) + "px";
      el.style.top = Math.max(0, Math.min(y, vh - h)) + "px";
    }
    function onKey(e) {
      const level = levels[levels.length - 1];
      if (!level) return;
      const k = e.key;
      const swallow = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (k === "Escape") {
        swallow();
        if (levels.length > 1) closeTo(levels.length - 1);
        else finish(null);
      } else if (k === "ArrowDown") {
        swallow();
        setActive(level, move(level, 1, level.active < 0 ? -1 : level.active));
      } else if (k === "ArrowUp") {
        swallow();
        setActive(level, move(level, -1, level.active < 0 ? 0 : level.active));
      } else if (k === "Home" || k === "End") {
        swallow();
        setActive(level, k === "Home" ? move(level, 1, -1) : move(level, -1, 0));
      } else if (k === "ArrowRight") {
        swallow();
        const item = level.items[level.active];
        if (item && item.items && item.items.length && selectable(item)) {
          const sub = openLevel(item.items, level, level.active);
          setActive(sub, move(sub, 1, -1));
        }
      } else if (k === "ArrowLeft") {
        swallow();
        if (levels.length > 1) closeTo(levels.length - 1);
      } else if (k === "Enter" || k === " ") {
        swallow();
        if (level.active >= 0) choose(level, level.active);
        if (levels.length && levels[levels.length - 1] !== level) {
          const sub = levels[levels.length - 1];
          setActive(sub, move(sub, 1, -1));
        }
      } else if (k === "Tab") {
        finish(null);
      } else if (k.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        const n = level.items.length;
        for (let d = 1; d <= n; d++) {
          const i = (Math.max(0, level.active) + d) % n;
          const item = level.items[i];
          if (selectable(item) && String(item.label).toLowerCase().startsWith(k.toLowerCase())) {
            swallow();
            setActive(level, i);
            break;
          }
        }
      }
    }
    document.addEventListener("mousedown", outside, true);
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("wheel", wheel, true);
    const first = openLevel(Array.isArray(list) ? list : [], null, -1);
    // With a keyboard-opened menu nothing is highlighted until an arrow key; the first row is the natural start.
    if (o.event === undefined && first.items.length && o.anchor) setActive(first, move(first, 1, -1));
  });
}

/** Show `items` (an array, or `(event) => array`) on the right button over `el`. Returns a function that removes it. */
export function contextMenu(el, items, options) {
  const handler = (e) => {
    const list = typeof items === "function" ? items(e) : items;
    if (!list || !list.length) return;
    e.preventDefault();
    menu(list, Object.assign({ event: e }, options || {}));
  };
  el.addEventListener("contextmenu", handler);
  return () => el.removeEventListener("contextmenu", handler);
}
