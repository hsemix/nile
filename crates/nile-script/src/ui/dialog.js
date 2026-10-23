// @nile/ui/dialog: <nile-dialog> and the promise helpers confirm(), prompt(), alert(), dialog().
//
//   if (await confirm("Void this order?", { ok: "Void", danger: true })) voidOrder();
//   const name = await prompt("Customer name", { value: "Walk-in" });      // string, or null when cancelled
//   await alert("Saved.");
//   const choice = await dialog({ title: "Print", message: "Print the receipt?",
//     buttons: [{ label: "No", value: "no", cancel: true }, { label: "Print", value: "yes", primary: true }] });
//
// Markup form (the content is yours; it gets a header, a scrolling body and a footer):
//
//   <nile-dialog id="d" heading="Edit item" width="420"><label>Qty <input></label>
//     <footer><button data-dialog-close>Cancel</button><button data-dialog-close="ok" class="nile-primary">OK</button></footer>
//   </nile-dialog>
//   const value = await d.showModal();      // the clicked button's data-dialog-close value, "" or undefined if dismissed
//
// Attributes: heading, width (px or CSS length), open, dismissible (default on: Escape and a click on the backdrop
// dismiss it). Methods: showModal() (a Promise), show(), close(value). Property: returnValue. Events: `open`, `close`,
// and a cancelable `beforeclose`. Escape dismisses, Enter presses the primary button, Tab stays inside, and focus
// goes back to where it was. Style with --nile-dialog-bg / -color / -shadow / -radius / -backdrop or the
// .nile-dialog-panel, .nile-dialog-head, .nile-dialog-body, .nile-dialog-foot classes.

import { style, emit, focusable, flagAttr, lengthAttr, modal } from "@nile/ui/base";

const CSS = `
:where(nile-dialog) { display: none; position: fixed; inset: 0; z-index: 1100; align-items: center; justify-content: center; }
:where(nile-dialog.nile-shown) { display: flex; }
:where(nile-dialog) > .nile-dialog-backdrop { position: absolute; inset: 0; background: var(--nile-dialog-backdrop, rgba(0, 0, 0, 0.45)); opacity: 0; transition: opacity 0.15s ease; }
:where(nile-dialog.nile-open) > .nile-dialog-backdrop { opacity: 1; }
:where(nile-dialog) > .nile-dialog-panel { position: relative; box-sizing: border-box; display: flex; flex-direction: column; min-width: 300px; max-width: 92%; max-height: 88%;
  background: var(--nile-dialog-bg, Canvas); color: var(--nile-dialog-color, CanvasText); border-radius: var(--nile-dialog-radius, 8px);
  box-shadow: var(--nile-dialog-shadow, 0 12px 40px rgba(0, 0, 0, 0.4)); outline: none; opacity: 0; transform: scale(0.97); transition: opacity 0.15s ease, transform 0.15s ease; }
:where(nile-dialog.nile-open) > .nile-dialog-panel { opacity: 1; transform: none; }
:where(.nile-dialog-head) { padding: 16px 20px 4px; font-size: 18px; font-weight: 600; }
:where(.nile-dialog-body) { padding: 10px 20px; overflow: auto; flex: 1 1 auto; min-height: 0; }
:where(.nile-dialog-text) { white-space: pre-wrap; line-height: 1.45; }
:where(.nile-dialog-body) > input { width: 100%; margin-top: 10px; box-sizing: border-box; }
:where(.nile-dialog-error) { color: #d33; font-size: 13px; margin-top: 6px; }
:where(.nile-dialog-foot), :where(nile-dialog > .nile-dialog-panel > footer) { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 20px 16px; }
:where(nile-dialog button.nile-primary) { background: var(--nile-primary, #2563eb); color: #fff; border-color: transparent; }
:where(nile-dialog button.nile-danger) { background: var(--nile-danger, #d33); color: #fff; border-color: transparent; }
`;

const stack = [];

export function defineDialog(tag) {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  const name = tag || "nile-dialog";
  if (customElements.get && customElements.get(name)) return;
  style("dialog", CSS);

  class NileDialog extends HTMLElement {
    static get observedAttributes() {
      return ["open", "heading", "width"];
    }
    connectedCallback() {
      if (!this._panel) this._build();
      this._head();
      this._width();
      if (this.hasAttribute("open") && !this._opened) this._reveal();
    }
    disconnectedCallback() {
      this._leave();
    }
    attributeChangedCallback(attr) {
      if (!this._panel) return;
      if (attr === "open") {
        if (this.hasAttribute("open")) this._reveal();
        else this._conceal();
      } else if (attr === "heading") this._head();
      else this._width();
    }
    _build() {
      const backdrop = document.createElement("div");
      backdrop.className = "nile-dialog-backdrop";
      const panel = document.createElement("div");
      panel.className = "nile-dialog-panel";
      panel.setAttribute("role", "dialog");
      panel.setAttribute("aria-modal", "true");
      panel.setAttribute("tabindex", "-1");
      const head = document.createElement("div");
      head.className = "nile-dialog-head";
      const body = document.createElement("div");
      body.className = "nile-dialog-body";
      let foot = null;
      Array.from(this.childNodes).forEach((n) => {
        if (n.nodeType === 1 && n.tagName === "FOOTER") foot = n;
        else body.appendChild(n);
      });
      panel.appendChild(head);
      panel.appendChild(body);
      if (foot) panel.appendChild(foot);
      this.appendChild(backdrop);
      this.appendChild(panel);
      this._panel = panel;
      this._headEl = head;
      this._body = body;
      this._opened = false;
      backdrop.addEventListener("mousedown", () => {
        if (flagAttr(this, "dismissible", true)) this.close(undefined);
      });
      panel.addEventListener("click", (e) => {
        const t = e.target && e.target.closest ? e.target.closest("[data-dialog-close]") : null;
        if (t && panel.contains(t)) this.close(t.getAttribute("data-dialog-close"));
      });
    }
    _head() {
      const text = this.getAttribute("heading") || "";
      this._headEl.textContent = text;
      this._headEl.style.display = text ? "" : "none";
      if (text) this._panel.setAttribute("aria-label", text);
    }
    _width() {
      this._panel.style.width = lengthAttr(this, "width", "");
    }
    get opened() {
      return this._opened === true;
    }
    get returnValue() {
      return this._value;
    }
    show() {
      this.setAttribute("open", "");
      return this;
    }
    showModal() {
      const done = new Promise((resolve) => {
        this._resolve = resolve;
      });
      this.show();
      return done;
    }
    /** Close, resolving showModal() with `value`. A `beforeclose` listener can veto it. */
    close(value) {
      if (!this._opened) return this;
      if (!emit(this, "beforeclose", { value: value }, true)) return this;
      this._value = value;
      this.removeAttribute("open");
      return this;
    }
    _reveal() {
      if (this._opened) return;
      this._opened = true;
      this._before = document.activeElement;
      clearTimeout(this._timer);
      this.classList.add("nile-shown");
      stack.push(this);
      modal.depth++;
      this._timer = setTimeout(() => {
        this.classList.add("nile-open");
        const target = this._panel.querySelector("[autofocus]") || this._panel.querySelector("input, textarea, select") || this._primary() || focusable(this._panel)[0] || this._panel;
        if (target && target.focus) target.focus();
        if (target && target.select && target.tagName === "INPUT") target.select();
      }, 20);
      emit(this, "open", null, false);
    }
    _leave() {
      const at = stack.indexOf(this);
      if (at >= 0) {
        stack.splice(at, 1);
        modal.depth = Math.max(0, modal.depth - 1);
      }
    }
    _conceal() {
      if (!this._opened) return;
      this._opened = false;
      this._leave();
      clearTimeout(this._timer);
      this.classList.remove("nile-open");
      const value = this._value;
      this._timer = setTimeout(() => {
        this.classList.remove("nile-shown");
        if (this.hasAttribute("data-nile-temp") && this.parentNode) this.parentNode.removeChild(this);
      }, 160);
      const before = this._before;
      this._before = null;
      if (before && before.focus && before.isConnected !== false) before.focus();
      emit(this, "close", { value: value }, false);
      const resolve = this._resolve;
      this._resolve = null;
      if (resolve) resolve(value);
    }
    _primary() {
      return this._panel.querySelector("button.nile-primary, button[data-default], button[type=submit]");
    }
  }
  customElements.define(name, NileDialog);
}

let wired = false;
function wire() {
  if (wired || typeof document === "undefined") return;
  wired = true;
  document.addEventListener(
    "keydown",
    (e) => {
      if (!stack.length) return;
      const top = stack[stack.length - 1];
      if (e.key === "Escape") {
        if (flagAttr(top, "dismissible", true)) top.close(undefined);
        e.preventDefault();
      } else if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
        const t = e.target;
        const tag = t && t.tagName;
        if (tag === "TEXTAREA" || tag === "SELECT" || (tag === "A" && t.getAttribute("href"))) return;
        // A focused button of the dialog is pressed (once: the default action is cancelled).
        if (tag === "BUTTON" && top._panel.contains(t)) {
          if (!t.disabled) t.click();
          e.preventDefault();
          return;
        }
        if (tag === "BUTTON") return;
        const primary = top._primary();
        if (primary && !primary.disabled) {
          primary.click();
          e.preventDefault();
        }
      } else if (e.key === "Tab") {
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

function button(spec, close) {
  const b = document.createElement("button");
  b.textContent = spec.label;
  if (spec.primary) b.className = "nile-primary";
  if (spec.danger) b.className = (b.className ? b.className + " " : "") + "nile-danger";
  if (spec.primary) b.setAttribute("data-default", "");
  b.addEventListener("click", () => close(spec));
  return b;
}

/**
 * Show a dialog built from options and resolve with the chosen button's value (undefined if dismissed).
 * Options: title, message (plain text), body (a Node, or { html }), buttons [{ label, value, primary, danger, cancel }]
 * (a value may be a function, read when the button is pressed), width, dismissible, check(value, button) (return false or
 * an error text to keep the dialog open).
 */
export function dialog(options) {
  defineDialog();
  wire();
  const o = options || {};
  const el = document.createElement("nile-dialog");
  el.setAttribute("data-nile-temp", "");
  if (o.title) el.setAttribute("heading", o.title);
  if (o.width) el.setAttribute("width", String(o.width));
  if (o.dismissible === false) el.setAttribute("dismissible", "false");
  if (o.message !== undefined && o.message !== null) {
    const text = document.createElement("div");
    text.className = "nile-dialog-text";
    text.textContent = String(o.message);
    el.appendChild(text);
  }
  if (o.body) {
    if (o.body.nodeType) el.appendChild(o.body);
    else if (o.body.html !== undefined) {
      const holder = document.createElement("div");
      holder.innerHTML = o.body.html;
      el.appendChild(holder);
    }
  }
  const error = document.createElement("div");
  error.className = "nile-dialog-error";
  error.setAttribute("role", "alert");
  error.style.display = "none";
  el.appendChild(error);
  const foot = document.createElement("footer");
  foot.className = "nile-dialog-foot";
  const buttons = o.buttons && o.buttons.length ? o.buttons : [{ label: "OK", value: true, primary: true }];
  buttons.forEach((spec) =>
    foot.appendChild(
      button(spec, (s) => {
        const value = typeof s.value === "function" ? s.value() : s.value;
        if (o.check && !s.cancel) {
          const verdict = o.check(value, s);
          if (verdict === false || typeof verdict === "string") {
            error.textContent = typeof verdict === "string" ? verdict : "";
            error.style.display = "";
            return;
          }
        }
        el.close(value);
      }),
    ),
  );
  el.appendChild(foot);
  document.body.appendChild(el);
  return el.showModal();
}

/** Ask a yes/no question. Resolves true or false. */
export function confirm(message, options) {
  const o = options || {};
  return dialog({
    title: o.title,
    message: message,
    width: o.width,
    dismissible: o.dismissible,
    buttons: [
      { label: o.cancel || "Cancel", value: false, cancel: true },
      { label: o.ok || "OK", value: true, primary: true, danger: !!o.danger },
    ],
  }).then((v) => v === true);
}

/** Show a message with one OK button. Resolves when closed. */
export function alert(message, options) {
  const o = options || {};
  return dialog({ title: o.title, message: message, width: o.width, buttons: [{ label: o.ok || "OK", value: true, primary: true }] }).then(() => undefined);
}

/** Ask for a line of text. Resolves the string, or null when cancelled. `validate(text)` may return an error text. */
export function prompt(message, options) {
  const o = options || {};
  const input = document.createElement("input");
  input.value = o.value === undefined ? "" : String(o.value);
  if (o.placeholder) input.setAttribute("placeholder", o.placeholder);
  if (o.type) input.setAttribute("type", o.type);
  input.style.width = "100%";
  input.style.boxSizing = "border-box";
  input.style.marginTop = "10px";
  const body = document.createElement("div");
  const text = document.createElement("div");
  text.className = "nile-dialog-text";
  text.textContent = String(message === undefined ? "" : message);
  body.appendChild(text);
  body.appendChild(input);
  return dialog({
    title: o.title,
    body: body,
    width: o.width,
    check: o.validate ? (v) => o.validate(v) || true : undefined,
    buttons: [
      { label: o.cancel || "Cancel", value: null, cancel: true },
      { label: o.ok || "OK", value: () => input.value, primary: true },
    ],
  }).then((v) => (v === undefined ? null : v));
}

defineDialog();
wire();
