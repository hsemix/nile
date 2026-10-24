// @nile/ui/wizard: <nile-wizard>, a step-by-step flow with a progress header and Back / Next / Finish buttons.
//
//   <nile-wizard id="setup" linear>
//     <nile-step label="Company" description="Name and tax number">...fields...</nile-step>
//     <nile-step label="Taxes" optional>...</nile-step>
//     <nile-step label="Done">...</nile-step>
//   </nile-wizard>
//   setup.addEventListener("finish", () => save());
//   setup.steps[0].validate = () => (name.value ? true : "Enter a company name");     // false, or text, keeps you here
//
// On <nile-wizard>: `selected` (the step index, 0), `linear` (later steps can't be clicked in the header until the
// earlier ones were visited), `back-label` / `next-label` / `finish-label` / `cancel-label` (button texts; a
// `cancel-label` adds a Cancel button), `hide-header`. On <nile-step>: `label`, `description`, `optional`
// (shown in the header; it can be skipped), `disabled` (skipped), and the `validate` property: a function that
// returns true, false, or an error text (a string keeps you on the step and shows it), or a Promise of those; it runs when
// you leave the step forwards. Methods: next(), back(), goto(index), reset(), finish(); properties: selectedIndex,
// steps, current. Events: cancelable `beforestepchange` (detail: { from, to }), `stepchange`, `finish` (cancelable
// `beforefinish` first), `cancel`. The header shows done steps with a check mark, the current one highlighted. Keyboard:
// Enter in a field presses Next (Finish on the last step) except in textareas and buttons. Style with
// --nile-wizard-accent, --nile-wizard-border, --nile-wizard-done or the .nile-wizard-header, .nile-wizard-dot
// ([aria-current], .nile-done), .nile-wizard-body, .nile-wizard-footer, .nile-wizard-error classes.

import { style, emit } from "@nile/ui/base";

const CSS = `
:where(nile-wizard) { display: flex; flex-direction: column; min-height: 0; }
:where(.nile-wizard-header) { display: flex; gap: 8px; padding: 12px 16px; border-bottom: 1px solid var(--nile-wizard-border, rgba(127, 127, 127, 0.4)); }
:where(nile-wizard) .nile-wizard-dot { flex: 1; display: flex; align-items: center; gap: 10px; min-width: 0; padding: 4px 0; border: 0; background: transparent; color: inherit; font: inherit; text-align: left; cursor: pointer; box-shadow: none; border-radius: 0; opacity: 0.7; }
:where(nile-wizard) .nile-wizard-dot[aria-current="step"] { opacity: 1; }
:where(nile-wizard) .nile-wizard-dot.nile-done { opacity: 0.9; }
:where(nile-wizard) .nile-wizard-dot[disabled] { cursor: default; opacity: 0.55; }
:where(.nile-wizard-num) { flex: none; width: 26px; height: 26px; border-radius: 13px; border: 2px solid currentColor; display: flex; align-items: center; justify-content: center; font-size: 13px; font-weight: 600; }
:where(nile-wizard) .nile-wizard-dot[aria-current="step"] .nile-wizard-num { background: var(--nile-wizard-accent, #2563eb); border-color: var(--nile-wizard-accent, #2563eb); color: #fff; }
:where(nile-wizard) .nile-wizard-dot.nile-done .nile-wizard-num { background: var(--nile-wizard-done, #16a34a); border-color: var(--nile-wizard-done, #16a34a); color: #fff; }
:where(.nile-wizard-text) { flex: 1; min-width: 0; display: flex; flex-direction: column; }
:where(.nile-wizard-title) { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
:where(.nile-wizard-sub) { font-size: 12px; opacity: 0.7; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
:where(nile-wizard) > .nile-wizard-body { flex: 1; min-height: 0; overflow: auto; padding: 16px; }
:where(nile-step) { display: none; }
:where(nile-step.nile-active) { display: block; }
:where(.nile-wizard-error) { padding: 0 16px; min-height: 18px; color: var(--nile-wizard-error, #dc2626); font-size: 13px; }
:where(.nile-wizard-footer) { display: flex; gap: 8px; justify-content: flex-end; padding: 10px 16px; border-top: 1px solid var(--nile-wizard-border, rgba(127, 127, 127, 0.4)); }
:where(.nile-wizard-footer) > .nile-wizard-spacer { flex: 1; }
:where(.nile-wizard-footer) button { min-width: 84px; padding: 7px 16px; }
:where(.nile-wizard-footer) button.nile-primary { background: var(--nile-wizard-accent, #2563eb); border-color: var(--nile-wizard-accent, #2563eb); color: #fff; }
`;

export function defineWizard() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-wizard")) return;
  style("wizard", CSS);

  class NileStep extends HTMLElement {
    attributeChangedCallback() {
      const host = this.closest ? this.closest("nile-wizard") : null;
      if (host && host._header) host._paintHeader();
    }
    static get observedAttributes() {
      return ["label", "description", "optional", "disabled"];
    }
    connectedCallback() {
      this.setAttribute("role", "group");
    }
  }

  class NileWizard extends HTMLElement {
    static get observedAttributes() {
      return ["selected", "back-label", "next-label", "finish-label", "cancel-label", "hide-header"];
    }
    get _st() {
      return this._state || (this._state = { built: false, at: 0, visited: new Set([0]), busy: false });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      this._header = document.createElement("div");
      this._header.className = "nile-wizard-header";
      this._header.setAttribute("role", "list");
      this._body = document.createElement("div");
      this._body.className = "nile-wizard-body";
      Array.from(this.childNodes).forEach((n) => {
        if (n.nodeType === 1 && n.tagName === "NILE-STEP") this._body.appendChild(n);
      });
      this._error = document.createElement("div");
      this._error.className = "nile-wizard-error";
      this._error.setAttribute("role", "alert");
      this._footer = document.createElement("div");
      this._footer.className = "nile-wizard-footer";
      this.insertBefore(this._header, this.firstChild);
      this.appendChild(this._body);
      this.appendChild(this._error);
      this.appendChild(this._footer);
      this._header.addEventListener("click", (e) => {
        const b = e.target.closest && e.target.closest(".nile-wizard-dot");
        if (b && !b.disabled) this.goto(Number(b.getAttribute("data-index")));
      });
      this.addEventListener("keydown", (e) => {
        if (e.key !== "Enter" || e.defaultPrevented) return;
        const t = e.target;
        const tag = t && t.tagName ? t.tagName.toLowerCase() : "";
        if (tag === "textarea" || tag === "button" || tag === "select" || tag === "a" || (t.closest && t.closest("nile-combobox, nile-date, nile-date-range, nile-calendar"))) return;
        if (this._body.contains(t)) {
          e.preventDefault();
          this._advance();
        }
      });
      const want = Number(this.getAttribute("selected"));
      st.at = Number.isInteger(want) && want >= 0 && want < this.steps.length ? want : 0;
      for (let i = 0; i <= st.at; i++) st.visited.add(i);
      this._paintHeader();
      this._paintFooter();
      this._show(false);
    }
    attributeChangedCallback(attr, was, now) {
      if (!this._st.built || was === now) return;
      if (attr === "selected") this.goto(Number(now));
      else {
        this._paintHeader();
        this._paintFooter();
      }
    }
    get steps() {
      return this._body ? Array.from(this._body.children).filter((n) => n.tagName === "NILE-STEP") : [];
    }
    get selectedIndex() {
      return this._st.at;
    }
    get current() {
      return this.steps[this._st.at] || null;
    }
    _label(step, i) {
      return step.getAttribute("label") || "Step " + (i + 1);
    }
    _enabled(i) {
      const s = this.steps[i];
      return !!s && !s.hasAttribute("disabled");
    }
    _paintHeader() {
      const st = this._st;
      if (!this._header) return;
      this._header.style.display = this.hasAttribute("hide-header") ? "none" : "";
      this._header.innerHTML = "";
      this.steps.forEach((step, i) => {
        const b = document.createElement("button");
        b.className = "nile-wizard-dot" + (i < st.at && st.visited.has(i) ? " nile-done" : "");
        b.setAttribute("type", "button");
        b.setAttribute("role", "listitem");
        b.setAttribute("data-index", String(i));
        if (i === st.at) b.setAttribute("aria-current", "step");
        // A linear wizard lets you go back, and forward only to steps already reached.
        if (!this._enabled(i) || (this.hasAttribute("linear") && i > st.at && !st.visited.has(i))) b.setAttribute("disabled", "");
        const num = document.createElement("span");
        num.className = "nile-wizard-num";
        num.textContent = i < st.at && st.visited.has(i) ? "✓" : String(i + 1);
        const text = document.createElement("span");
        text.className = "nile-wizard-text";
        const title = document.createElement("span");
        title.className = "nile-wizard-title";
        title.textContent = this._label(step, i);
        text.appendChild(title);
        const sub = step.hasAttribute("optional") ? "Optional" : step.getAttribute("description");
        if (sub) {
          const d = document.createElement("span");
          d.className = "nile-wizard-sub";
          d.textContent = sub;
          text.appendChild(d);
        }
        b.appendChild(num);
        b.appendChild(text);
        this._header.appendChild(b);
      });
    }
    _paintFooter() {
      const st = this._st;
      if (!this._footer) return;
      this._footer.innerHTML = "";
      const make = (text, cls, fn) => {
        const b = document.createElement("button");
        b.setAttribute("type", "button");
        if (cls) b.className = cls;
        b.textContent = text;
        b.addEventListener("click", fn);
        this._footer.appendChild(b);
        return b;
      };
      if (this.hasAttribute("cancel-label")) {
        make(this.getAttribute("cancel-label"), "nile-wizard-cancel", () => emit(this, "cancel", {}, false));
        const spacer = document.createElement("span");
        spacer.className = "nile-wizard-spacer";
        this._footer.appendChild(spacer);
      }
      const last = this._nextIndex(1) < 0;
      this._back = make(this.getAttribute("back-label") || "Back", "nile-wizard-back", () => this.back());
      this._back.disabled = this._nextIndex(-1) < 0 || st.busy;
      this._next = make(last ? this.getAttribute("finish-label") || "Finish" : this.getAttribute("next-label") || "Next", "nile-primary nile-wizard-next", () => this._advance());
      this._next.disabled = st.busy;
    }
    /** The nearest enabled step in `dir` from the current one, or -1. */
    _nextIndex(dir) {
      const n = this.steps.length;
      for (let i = this._st.at + dir; i >= 0 && i < n; i += dir) if (this._enabled(i)) return i;
      return -1;
    }
    _show(notify, from) {
      const st = this._st;
      this.steps.forEach((s, i) => {
        if (i === st.at) s.classList.add("nile-active");
        else s.classList.remove("nile-active");
      });
      this._error.textContent = "";
      this._paintHeader();
      this._paintFooter();
      if (notify) emit(this, "stepchange", { from: from, to: st.at, step: this.current }, false);
    }
    _setBusy(on) {
      this._st.busy = on;
      if (this._next) this._next.disabled = on;
      if (this._back) this._back.disabled = on || this._nextIndex(-1) < 0;
    }
    /** Run the leaving step's validation: resolves true to go on, false (with the text shown) to stay. */
    async _valid(step) {
      if (!step || typeof step.validate !== "function") return true;
      this._setBusy(true);
      try {
        const verdict = await step.validate(step);
        if (verdict === false || (typeof verdict === "string" && verdict)) {
          this._error.textContent = typeof verdict === "string" ? verdict : "Check this step before going on.";
          return false;
        }
        return true;
      } finally {
        this._setBusy(false);
      }
    }
    _advance() {
      return this._nextIndex(1) < 0 ? this.finish() : this.next();
    }
    async next() {
      const to = this._nextIndex(1);
      if (to < 0 || this._st.busy) return false;
      return this.goto(to);
    }
    async back() {
      const to = this._nextIndex(-1);
      if (to < 0 || this._st.busy) return false;
      return this.goto(to);
    }
    /** Go to step `index`. Moving forwards validates the steps you leave. Resolves true when it got there. */
    async goto(index) {
      const st = this._st;
      const to = Number(index);
      if (!st.built || st.busy || !Number.isInteger(to) || to < 0 || to >= this.steps.length || to === st.at || !this._enabled(to)) return false;
      const from = st.at;
      if (to > from) {
        for (let i = from; i < to; i++) {
          if (i !== from && !this._enabled(i)) continue;
          if (!(await this._valid(this.steps[i]))) {
            if (i !== from) {
              st.at = i;
              this._show(true, from);
            }
            return false;
          }
        }
      }
      if (!emit(this, "beforestepchange", { from: from, to: to }, true)) return false;
      st.at = to;
      st.visited.add(to);
      this._show(true, from);
      return true;
    }
    async finish() {
      const st = this._st;
      if (st.busy) return false;
      if (!(await this._valid(this.current))) return false;
      if (!emit(this, "beforefinish", {}, true)) return false;
      emit(this, "finish", {}, false);
      return true;
    }
    /** Back to the first step and forget which were visited. */
    reset() {
      const st = this._st;
      st.at = 0;
      st.visited = new Set([0]);
      if (st.built) this._show(false);
      return this;
    }
  }
  customElements.define("nile-step", NileStep);
  customElements.define("nile-wizard", NileWizard);
}

defineWizard();
