// @nile/ui/shortcuts: keyboard shortcuts for the whole window.
//
//   import { shortcuts } from "@nile/ui";
//   const off = shortcuts.on("F3", () => openSearch());            // returns a function that removes it
//   shortcuts.on("Mod+S", save, { label: "Save" });                // Mod = Ctrl, or Cmd on a Mac
//   shortcuts.on("Ctrl+Shift+K, Alt+K", clear);                    // several keys for one action
//   <button data-shortcut="F8">New sale</button>                   // pressing F8 clicks the button
//
// Key names: letters, digits, F1-F12, Enter, Escape, Delete, Backspace, Tab, Space, ArrowUp/Down/Left/Right, Home,
// End, PageUp, PageDown, plus the modifiers Ctrl, Alt, Shift, Meta, Mod joined with "+". Shortcuts fire when focus is
// anywhere, except that a plain letter, digit or punctuation key (no Ctrl/Alt/Meta) is left to the text field you are
// typing in. They stay quiet while a dialog is open unless the shortcut is registered with { always: true }.
// { when: () => boolean } adds a condition; { prevent: false } lets the browser default run too.
// shortcuts.list() returns what is registered; shortcuts.format("Mod+S") gives "Ctrl+S" or "⌘S" for display
// (data-shortcut buttons get it in their `data-hint` attribute).

import { modal } from "@nile/ui/base";
import { PLATFORM } from "@env";

const mac = PLATFORM === "OSX";
const entries = [];
let wired = false;

const ALIASES = { esc: "escape", del: "delete", return: "enter", " ": "space", spacebar: "space", up: "arrowup", down: "arrowdown", left: "arrowleft", right: "arrowright", plus: "+", ins: "insert" };

function norm(key) {
  const k = String(key).toLowerCase();
  return ALIASES[k] || k;
}

/** "Ctrl+Shift+K" -> { ctrl, shift, alt, meta, key } */
function parse(text) {
  const out = { ctrl: false, shift: false, alt: false, meta: false, key: "" };
  const parts = String(text).split("+").map((p) => p.trim());
  // A trailing "+" key ("Ctrl++") leaves an empty last part.
  if (parts.length > 1 && parts[parts.length - 1] === "") parts[parts.length - 1] = "+";
  parts.forEach((p) => {
    const k = norm(p);
    if (k === "ctrl" || k === "control") out.ctrl = true;
    else if (k === "shift") out.shift = true;
    else if (k === "alt" || k === "option") out.alt = true;
    else if (k === "meta" || k === "cmd" || k === "command" || k === "win") out.meta = true;
    else if (k === "mod") {
      if (mac) out.meta = true;
      else out.ctrl = true;
    } else if (k) out.key = k;
  });
  return out;
}

function chords(text) {
  return String(text).split(",").map((c) => c.trim()).filter(Boolean).map(parse);
}

function matches(chord, e) {
  if (!chord.key || norm(e.key) !== chord.key) return false;
  return !!e.ctrlKey === chord.ctrl && !!e.shiftKey === chord.shift && !!e.altKey === chord.alt && !!e.metaKey === chord.meta;
}

/** A key a text field uses for typing: printable, no Ctrl/Alt/Meta. */
function isTyping(e) {
  if (e.ctrlKey || e.altKey || e.metaKey) return false;
  const t = e.target;
  const tag = t && t.tagName;
  const editable = tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || (t && t.isContentEditable);
  return !!editable && String(e.key).length === 1;
}

function visible(el) {
  if (!el.isConnected || el.disabled || el.hasAttribute("hidden")) return false;
  const r = el.getBoundingClientRect ? el.getBoundingClientRect() : null;
  return !r || r.width > 0 || r.height > 0;
}

function onKey(e) {
  if (e.isComposing || e.repeat) return;
  const typing = isTyping(e);
  const blocked = modal.depth > 0;
  for (const entry of entries.slice()) {
    if (typing && !entry.typing) continue;
    if (blocked && !entry.always) continue;
    if (!entry.chords.some((c) => matches(c, e))) continue;
    if (entry.when && !entry.when(e)) continue;
    if (entry.prevent) e.preventDefault();
    entry.run(e);
    return;
  }
  if (typing || blocked) return;
  const tagged = document.querySelectorAll("[data-shortcut]");
  for (let i = 0; i < tagged.length; i++) {
    const el = tagged[i];
    if (!chords(el.getAttribute("data-shortcut")).some((c) => matches(c, e)) || !visible(el)) continue;
    e.preventDefault();
    el.click();
    return;
  }
}

function wire() {
  if (wired || typeof document === "undefined") return;
  wired = true;
  document.addEventListener("keydown", onKey);
  hint();
}

/** Show each data-shortcut in the element's data-hint attribute, so a stylesheet can display it. */
function hint() {
  const tagged = document.querySelectorAll("[data-shortcut]");
  for (let i = 0; i < tagged.length; i++) {
    const el = tagged[i];
    el.setAttribute("data-hint", formatShortcut(el.getAttribute("data-shortcut").split(",")[0]));
  }
}

export function formatShortcut(text) {
  const c = parse(text);
  const key = c.key.length === 1 ? c.key.toUpperCase() : c.key.charAt(0).toUpperCase() + c.key.slice(1);
  const mods = [];
  if (c.ctrl) mods.push(mac ? "⌃" : "Ctrl");
  if (c.alt) mods.push(mac ? "⌥" : "Alt");
  if (c.shift) mods.push(mac ? "⇧" : "Shift");
  if (c.meta) mods.push(mac ? "⌘" : "Win");
  return mac ? mods.join("") + key : mods.concat(key).join("+");
}

export const shortcuts = {
  /** Register `keys` ("F3", "Mod+S", "A, B"); returns a function that removes it. */
  on(keys, run, options) {
    wire();
    const o = options || {};
    const entry = { keys: String(keys), chords: chords(keys), run: run, when: o.when, label: o.label, always: !!o.always, typing: !!o.typing, prevent: o.prevent !== false };
    entries.push(entry);
    return () => shortcuts.off(entry);
  },
  /** Remove one registration (the function `on` returned, or the handler you gave it). */
  off(what) {
    for (let i = entries.length - 1; i >= 0; i--) {
      if (entries[i] === what || entries[i].run === what) entries.splice(i, 1);
    }
  },
  list() {
    return entries.map((e) => ({ keys: e.keys, label: e.label || "" }));
  },
  format: formatShortcut,
  /** Re-read data-shortcut attributes after adding buttons (sets data-hint). */
  refresh: hint,
};

if (typeof document !== "undefined") {
  wire();
}
