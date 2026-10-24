// @nile/ui/palette: a command palette (Ctrl+K) and a general "type to pick" list.
//
//   import { palette } from "@nile/ui";
//   palette.add({ id: "new-sale", label: "New sale", group: "Sales", shortcut: "F8", run: newSale });
//   palette.bind("Mod+K");                     // opens the palette from anywhere (Mod = Ctrl, or Cmd on a Mac)
//   const customer = await palette.open({ items: customers.map((c) => ({ label: c.name, hint: c.phone, data: c })),
//                                         placeholder: "Find a customer..." });      // null when dismissed
//
// Commands: { id, label, hint (grey text on the right), group (heading), shortcut ("Mod+S", shown for you), keywords
// (extra words to match), disabled, run(command) }. `palette.add` returns a function that removes it;
// `palette.commands()` lists them. Without `items`, `open()` shows the commands you added plus every shortcut
// registered with a `label` (`shortcuts.on("Mod+S", save, { label: "Save" })`), so shortcuts you already have show
// up with no extra code. Matching is fuzzy: the typed letters must appear in order, with word starts and runs
// scoring higher, and the matched letters are underlined. Choosing runs the command's `run` (pass `run: false` to
// only get the answer back); the returned Promise resolves with the chosen item or null. Options: items, placeholder,
// empty (text when nothing matches), max (visible rows, 8), value (initial text), shortcuts (false = leave labelled
// shortcuts out). Keyboard: Up/Down, PageUp/PageDown, Enter, Escape; a click chooses. `palette.bind(keys)` returns a
// function that removes the binding; the shortcut toggles the palette. Style with --nile-palette-bg,
// --nile-palette-color, --nile-palette-hover, --nile-palette-border, --nile-palette-accent or the .nile-palette,
// .nile-palette-input, .nile-palette-row (.nile-active), .nile-palette-group classes.

import { style, modal } from "@nile/ui/base";
import { shortcuts, formatShortcut } from "@nile/ui/shortcuts";

const CSS = `
:where(.nile-palette-backdrop) { position: fixed; left: 0; top: 0; right: 0; bottom: 0; z-index: 1500; background: rgba(0, 0, 0, 0.35); display: flex; justify-content: center; align-items: flex-start; padding-top: 12%; box-sizing: border-box; }
:where(.nile-palette) { width: 560px; max-width: 92%; display: flex; flex-direction: column; background: var(--nile-palette-bg, Canvas); color: var(--nile-palette-color, CanvasText); border: 1px solid var(--nile-palette-border, rgba(127, 127, 127, 0.5)); border-radius: 8px; box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4); overflow: hidden; }
:where(.nile-palette) .nile-palette-input { width: 100%; box-sizing: border-box; padding: 12px 14px; border: 0; border-bottom: 1px solid var(--nile-palette-border, rgba(127, 127, 127, 0.4)); outline: none; background: transparent; color: inherit; font: inherit; font-size: 16px; border-radius: 0; box-shadow: none; }
:where(.nile-palette-list) { overflow-y: auto; padding: 4px 0; }
:where(.nile-palette-row) { display: flex; align-items: center; gap: 12px; padding: 7px 14px; cursor: pointer; }
:where(.nile-palette-row.nile-active) { background: var(--nile-palette-hover, rgba(37, 99, 235, 0.2)); }
:where(.nile-palette-row[aria-disabled="true"]) { opacity: 0.4; cursor: default; }
:where(.nile-palette-label) { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
:where(.nile-palette-hint) { flex: none; opacity: 0.6; font-size: 12px; }
:where(.nile-palette-key) { flex: none; padding: 1px 6px; border: 1px solid var(--nile-palette-border, rgba(127, 127, 127, 0.5)); border-radius: 4px; font-size: 11px; opacity: 0.75; }
:where(.nile-palette-group) { padding: 8px 14px 2px; font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; opacity: 0.55; }
:where(.nile-palette-empty) { padding: 14px; opacity: 0.6; text-align: center; }
:where(.nile-palette-match) { background: transparent; color: var(--nile-palette-accent, #2563eb); font-weight: 700; text-decoration: underline; }
`;

const commands = [];
let openState = null;

/** Fuzzy match: the letters of `query` in order. Returns { score, at: [indexes] } or null. */
export function fuzzy(query, text) {
  const q = String(query).toLowerCase().replace(/\s+/g, "");
  const t = String(text).toLowerCase();
  if (!q) return { score: 0, at: [] };
  // A contiguous hit beats a scattered one.
  const whole = t.indexOf(q);
  if (whole >= 0) {
    const at = [];
    for (let i = 0; i < q.length; i++) at.push(whole + i);
    return { score: 100 + (whole === 0 ? 50 : /[\s\-_/.]/.test(t[whole - 1] || " ") ? 25 : 0) - whole - t.length * 0.1, at: at };
  }
  const at = [];
  let score = 0;
  let from = 0;
  let last = -2;
  for (let i = 0; i < q.length; i++) {
    const found = t.indexOf(q[i], from);
    if (found < 0) return null;
    at.push(found);
    score += 1;
    if (found === last + 1) score += 4;
    if (found === 0 || /[\s\-_/.]/.test(t[found - 1])) score += 3;
    last = found;
    from = found + 1;
  }
  return { score: score - t.length * 0.05, at: at };
}

function labelled(label, at) {
  const frag = document.createDocumentFragment ? document.createDocumentFragment() : document.createElement("span");
  const set = new Set(at);
  let run = "";
  let marked = false;
  const flush = () => {
    if (!run) return;
    if (marked) {
      const m = document.createElement("span");
      m.className = "nile-palette-match";
      m.textContent = run;
      frag.appendChild(m);
    } else frag.appendChild(document.createTextNode(run));
    run = "";
  };
  Array.from(String(label)).forEach((ch, i) => {
    const hit = set.has(i);
    if (hit !== marked) {
      flush();
      marked = hit;
    }
    run += ch;
  });
  flush();
  return frag;
}

function everyCommand(o) {
  const out = commands.slice();
  if (o.shortcuts !== false) {
    const have = new Set(out.map((c) => String(c.label).toLowerCase()));
    shortcuts.list().forEach((s) => {
      if (s.label && !have.has(s.label.toLowerCase()) && typeof s.run === "function") out.push({ label: s.label, shortcut: s.keys.split(",")[0].trim(), run: s.run, group: "Shortcuts" });
    });
  }
  return out;
}

/** Open the palette. Resolves with the chosen item (after running it) or null. */
function open(options) {
  const o = options || {};
  style("palette", CSS);
  if (openState) {
    // Opening again (the toggle shortcut) closes it.
    openState.close(null);
    return Promise.resolve(null);
  }
  const items = (o.items ? o.items : everyCommand(o)).map((x) => (x !== null && typeof x === "object" ? x : { label: String(x) }));
  return new Promise((resolve) => {
    const opener = document.activeElement;
    const backdrop = document.createElement("div");
    backdrop.className = "nile-palette-backdrop";
    const box = document.createElement("div");
    box.className = "nile-palette";
    box.setAttribute("role", "dialog");
    box.setAttribute("aria-modal", "true");
    box.setAttribute("aria-label", o.placeholder || "Command palette");
    const input = document.createElement("input");
    input.className = "nile-palette-input";
    input.setAttribute("type", "text");
    input.setAttribute("autocomplete", "off");
    input.setAttribute("placeholder", o.placeholder || "Type a command...");
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-expanded", "true");
    input.value = o.value ? String(o.value) : "";
    const list = document.createElement("div");
    list.className = "nile-palette-list";
    list.setAttribute("role", "listbox");
    const rowH = 34;
    list.style.maxHeight = (o.max || 8) * rowH + 40 + "px";
    box.appendChild(input);
    box.appendChild(list);
    backdrop.appendChild(box);
    let shown = [];
    let at = 0;
    let done = false;
    modal.depth++;

    function finish(choice) {
      if (done) return;
      done = true;
      modal.depth = Math.max(0, modal.depth - 1);
      openState = null;
      backdrop.remove();
      if (opener && opener.focus && opener !== document.body) opener.focus();
      if (choice && o.run !== false && typeof choice.run === "function") choice.run(choice);
      resolve(choice || null);
    }
    openState = { close: finish };

    function paint() {
      const q = input.value;
      shown = [];
      items.forEach((item, i) => {
        const hay = String(item.label) + " " + (item.keywords || "") + " " + (item.group || "");
        const m = fuzzy(q, item.label);
        const loose = m || (item.keywords ? fuzzy(q, hay) : null);
        if (loose) shown.push({ item: item, score: loose.score + (m ? 0 : -20), at: m ? m.at : [], order: i });
      });
      // Without a query keep the given order; otherwise best match first.
      if (q.trim()) shown.sort((a, b) => b.score - a.score || a.order - b.order);
      at = Math.min(at, Math.max(0, shown.length - 1));
      list.innerHTML = "";
      if (!shown.length) {
        const empty = document.createElement("div");
        empty.className = "nile-palette-empty";
        empty.textContent = o.empty || "No matches";
        list.appendChild(empty);
        return;
      }
      let group = null;
      shown.forEach((s, i) => {
        const g = s.item.group;
        if (g && g !== group && !q.trim()) {
          const head = document.createElement("div");
          head.className = "nile-palette-group";
          head.textContent = g;
          list.appendChild(head);
        }
        group = g;
        const row = document.createElement("div");
        row.className = "nile-palette-row";
        row.setAttribute("role", "option");
        row.setAttribute("data-i", String(i));
        if (s.item.disabled) row.setAttribute("aria-disabled", "true");
        const label = document.createElement("span");
        label.className = "nile-palette-label";
        label.appendChild(labelled(s.item.label, s.at));
        row.appendChild(label);
        if (s.item.hint) {
          const hint = document.createElement("span");
          hint.className = "nile-palette-hint";
          hint.textContent = s.item.hint;
          row.appendChild(hint);
        }
        if (s.item.shortcut) {
          const key = document.createElement("span");
          key.className = "nile-palette-key";
          key.textContent = formatShortcut(s.item.shortcut);
          row.appendChild(key);
        }
        row.addEventListener("mousedown", (e) => e.preventDefault());
        row.addEventListener("click", () => {
          if (!s.item.disabled) finish(s.item);
        });
        row.addEventListener("mouseenter", () => mark(i));
        list.appendChild(row);
      });
      mark(at);
    }
    function mark(i) {
      at = i;
      Array.from(list.querySelectorAll(".nile-palette-row")).forEach((row) => {
        const on = Number(row.getAttribute("data-i")) === i;
        if (on) row.classList.add("nile-active");
        else row.classList.remove("nile-active");
        row.setAttribute("aria-selected", on ? "true" : "false");
        if (on) {
          const top = row.offsetTop, bottom = top + row.offsetHeight;
          if (top < list.scrollTop) list.scrollTop = top;
          else if (bottom > list.scrollTop + list.clientHeight) list.scrollTop = bottom - list.clientHeight;
        }
      });
    }
    input.addEventListener("input", () => {
      at = 0;
      paint();
    });
    input.addEventListener("keydown", (e) => {
      const n = shown.length;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        if (n) mark((at + 1) % n);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        if (n) mark((at - 1 + n) % n);
      } else if (e.key === "PageDown") {
        e.preventDefault();
        if (n) mark(Math.min(n - 1, at + 6));
      } else if (e.key === "PageUp") {
        e.preventDefault();
        if (n) mark(Math.max(0, at - 6));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const s = shown[at];
        if (s && !s.item.disabled) finish(s.item);
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        finish(null);
      } else if (e.key === "Tab") e.preventDefault();
    });
    backdrop.addEventListener("mousedown", (e) => {
      if (e.target === backdrop) finish(null);
    });
    document.body.appendChild(backdrop);
    paint();
    input.focus();
  });
}

export const palette = {
  /** Add a command; returns a function that removes it. */
  add(command) {
    const c = Object.assign({}, command);
    commands.push(c);
    return () => {
      const i = commands.indexOf(c);
      if (i >= 0) commands.splice(i, 1);
    };
  },
  commands() {
    return commands.slice();
  },
  open: open,
  /** Whether the palette is showing. */
  get isOpen() {
    return !!openState;
  },
  close() {
    if (openState) openState.close(null);
  },
  /** Open the palette from `keys` (default "Mod+K"); returns a function that removes the binding. */
  bind(keys) {
    return shortcuts.on(keys || "Mod+K", () => open(), { always: true, label: "" });
  },
};
