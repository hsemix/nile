// @nile/ui/keypad: <nile-keypad>, an on-screen number pad for quantities, prices, cash and PINs.
//
//   <input id="qty">
//   <nile-keypad target="#qty"></nile-keypad>                       // types into the input, which keeps focus
//   <nile-keypad pin max-length="4" display></nile-keypad>          // a PIN pad with its own masked display
//
// Keys: 0-9, `.` (when `decimal`), `00` (`double-zero`), a sign key (`sign`), Backspace, Clear and Enter. The first
// digit pressed after the pad is shown, cleared or entered replaces what was there (like a till); set
// `replace-first="false"` to append instead. Attributes: `target` (a selector, or set the `target` element), `decimal`,
// `decimals` (max digits after the point, default 2 with `decimal`), `max-length`, `max` (largest number), `pin`
// (mask the display), `display` (show a value line), `sign`, `double-zero`, `enter-label` (text of the Enter key,
// default "OK"), `keys` ("quick" amounts as a comma list, e.g. "5,10,20,50": each adds a quick key that adds it to the value).
// Properties: value (string), number (parsed, or NaN), target. Methods: press(key) (a digit, ".", "00", "-",
// "Backspace", "Clear" or "Enter"), clear(). Events: `input` (detail: { value, number }) on the pad (and on the
// target, as a normal `input` event), `enter` (detail: { value, number }; the pad's Enter key, cancelable
// `beforeinput` with detail { key }). Buttons keep keyboard focus where it was, so a pointer or touch screen can
// drive a field you are also typing in. Style with --nile-keypad-bg, --nile-keypad-key, --nile-keypad-key-hover,
// --nile-keypad-accent, --nile-keypad-gap or the .nile-keypad-key (data-key, .nile-enter) classes.

import { style, emit, flagAttr, numberAttr } from "@nile/ui/base";

const CSS = `
:where(nile-keypad) { display: inline-flex; flex-direction: column; gap: var(--nile-keypad-gap, 6px); padding: 8px; box-sizing: border-box; background: var(--nile-keypad-bg, transparent); user-select: none; }
:where(nile-keypad) .nile-keypad-display { box-sizing: border-box; min-height: 40px; padding: 6px 12px; text-align: right; font-size: 22px; font-variant-numeric: tabular-nums; letter-spacing: 0.05em; border: 1px solid var(--nile-keypad-border, rgba(127, 127, 127, 0.5)); border-radius: 6px; background: var(--nile-keypad-display, rgba(127, 127, 127, 0.08)); }
:where(nile-keypad) .nile-keypad-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--nile-keypad-gap, 6px); }
:where(nile-keypad) .nile-keypad-quick .nile-keypad-key { min-width: 0; padding: 6px 0; }
:where(nile-keypad) .nile-keypad-quick { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: var(--nile-keypad-gap, 6px); }
:where(nile-keypad) .nile-keypad-key { min-width: 56px; min-height: 48px; margin: 0; padding: 6px; border: 1px solid var(--nile-keypad-border, rgba(127, 127, 127, 0.45)); border-radius: 6px; background: var(--nile-keypad-key, rgba(127, 127, 127, 0.14)); color: inherit; font: inherit; font-size: 20px; cursor: pointer; box-shadow: none; }
:where(nile-keypad) .nile-keypad-key:hover { background: var(--nile-keypad-key-hover, rgba(127, 127, 127, 0.28)); }
:where(nile-keypad) .nile-keypad-key:active { background: var(--nile-keypad-accent, #2563eb); color: #fff; }
:where(nile-keypad) .nile-keypad-key.nile-enter { background: var(--nile-keypad-accent, #2563eb); color: #fff; font-size: 16px; font-weight: 600; }
:where(nile-keypad[disabled]) { opacity: 0.5; pointer-events: none; }
`;

export function defineKeypad() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-keypad")) return;
  style("keypad", CSS);

  class NileKeypad extends HTMLElement {
    static get observedAttributes() {
      return ["target", "decimal", "pin", "display", "sign", "double-zero", "keys", "enter-label"];
    }
    get _st() {
      return this._state || (this._state = { value: "", built: false, fresh: true, target: null, shown: null });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      this.setAttribute("role", "group");
      if (!this.hasAttribute("aria-label")) this.setAttribute("aria-label", "Number pad");
      this._build();
    }
    attributeChangedCallback(attr, was, now) {
      if (this._st.built && was !== now) {
        if (attr === "target") this._st.target = null;
        this._build();
      }
    }
    get value() {
      return this._st.value;
    }
    set value(v) {
      const st = this._st;
      st.value = v === null || v === undefined ? "" : String(v);
      st.fresh = true;
      this._show();
    }
    get number() {
      return this._st.value === "" || this._st.value === "-" || this._st.value === "." ? NaN : Number(this._st.value);
    }
    get target() {
      const st = this._st;
      if (st.target) return st.target;
      const sel = this.getAttribute("target");
      return sel ? document.querySelector(sel) : null;
    }
    set target(el) {
      this._st.target = el || null;
      this._st.fresh = true;
    }
    clear() {
      const st = this._st;
      st.value = "";
      st.fresh = true;
      this._send();
      return this;
    }

    _build() {
      const st = this._st;
      this.innerHTML = "";
      if (this.hasAttribute("display") || this.hasAttribute("pin")) {
        st.shown = document.createElement("div");
        st.shown.className = "nile-keypad-display";
        st.shown.setAttribute("role", "status");
        this.appendChild(st.shown);
      } else st.shown = null;
      const quick = (this.getAttribute("keys") || "").split(",").map((s) => s.trim()).filter(Boolean);
      if (quick.length) {
        const row = document.createElement("div");
        row.className = "nile-keypad-quick";
        quick.forEach((q) => row.appendChild(this._key("+" + q, "+" + q)));
        this.appendChild(row);
      }
      const grid = document.createElement("div");
      grid.className = "nile-keypad-grid";
      const decimal = flagAttr(this, "decimal", false) && !this.hasAttribute("pin");
      const layout = ["7", "8", "9", "4", "5", "6", "1", "2", "3"];
      layout.forEach((k) => grid.appendChild(this._key(k, k)));
      grid.appendChild(this._key(decimal ? "." : "Clear", decimal ? "." : "C"));
      grid.appendChild(this._key("0", "0"));
      grid.appendChild(this._key("Backspace", "⌫"));
      // Optional helper keys fill the next row; Enter takes what is left of it.
      const extra = [];
      if (decimal) extra.push(["Clear", "C"]);
      if (this.hasAttribute("sign")) extra.push(["-", "±"]);
      if (this.hasAttribute("double-zero")) extra.push(["00", "00"]);
      extra.forEach(([k, l]) => grid.appendChild(this._key(k, l)));
      const enter = this._key("Enter", this.getAttribute("enter-label") || "OK");
      enter.className += " nile-enter";
      enter.style.gridColumn = "span " + (3 - (extra.length % 3));
      grid.appendChild(enter);
      this.appendChild(grid);
      this._show();
    }
    _key(key, label) {
      const b = document.createElement("button");
      b.className = "nile-keypad-key";
      b.setAttribute("type", "button");
      b.setAttribute("tabindex", "-1");
      b.setAttribute("data-key", key);
      b.setAttribute("aria-label", key === "Backspace" ? "Backspace" : key === "Clear" ? "Clear" : key === "Enter" ? label : key === "-" ? "Plus or minus" : key);
      b.textContent = label;
      // Pressing a key must not take focus from the field being filled.
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.addEventListener("click", () => this.press(key));
      return b;
    }

    /** One key: a digit, ".", "00", "-", "+N" (quick amount), "Backspace", "Clear" or "Enter". */
    press(key) {
      const st = this._st;
      key = String(key);
      if (this.hasAttribute("disabled")) return this;
      if (!emit(this, "beforeinput", { key: key }, true)) return this;
      const maxLen = numberAttr(this, "max-length", Infinity);
      const decimals = numberAttr(this, "decimals", this.hasAttribute("decimal") ? 2 : 0);
      const target = this.target;
      // The first key replaces what the field held; with replace-first="false" the pad carries on from its text.
      const replace = flagAttr(this, "replace-first", true);
      if (st.fresh && !replace && target && st.value === "" && target.value) st.value = String(target.value);
      let v = st.value;
      if (key === "Enter") {
        emit(this, "enter", { value: v, number: this.number }, false);
        st.fresh = true;
        return this;
      } else if (key === "Clear") {
        v = "";
      } else if (key === "Backspace") {
        v = v.slice(0, -1);
        st.fresh = false;
      } else if (key === "-") {
        v = v.startsWith("-") ? v.slice(1) : "-" + v;
      } else if (key[0] === "+" && key.length > 1) {
        const add = Number(key.slice(1));
        const base = st.fresh && replace ? 0 : Number(v) || 0;
        v = String(Math.round((base + add) * 1e6) / 1e6);
        st.fresh = false;
      } else if (key === ".") {
        if (v.includes(".")) return this;
        v = (v === "" || v === "-" ? v + "0" : v) + ".";
        st.fresh = false;
      } else if (/^\d+$/.test(key)) {
        if (st.fresh && replace) v = "";
        st.fresh = false;
        const dot = v.indexOf(".");
        if (dot >= 0 && v.length - dot - 1 >= decimals) return this;
        if (v.replace(/[-.]/g, "").length + key.length > maxLen) return this;
        // No leading zeros: "0" then "5" is "5".
        if (v === "0" || v === "-0") v = v.slice(0, -1);
        v += key;
        const max = numberAttr(this, "max", Infinity);
        if (Number(v) > max) return this;
      } else return this;
      st.value = v;
      if (key === "Clear") st.fresh = true;
      this._send();
      return this;
    }
    _show() {
      const st = this._st;
      if (!st.shown) return;
      const pin = this.hasAttribute("pin");
      st.shown.textContent = pin ? "●".repeat(st.value.length) : st.value === "" ? "0" : st.value;
    }
    _send() {
      const st = this._st;
      this._show();
      const target = this.target;
      if (target) {
        target.value = st.value;
        target.dispatchEvent(new Event("input", { bubbles: true }));
      }
      emit(this, "input", { value: st.value, number: this.number }, false);
    }
  }
  customElements.define("nile-keypad", NileKeypad);
}

defineKeypad();
