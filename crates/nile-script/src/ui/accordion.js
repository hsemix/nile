// @nile/ui/accordion: collapsible sections, alone or in an <nile-accordion>.
//
//   <nile-accordion exclusive>
//     <nile-section heading="General" open>...links...</nile-section>
//     <nile-section heading="Message center">...</nile-section>
//   </nile-accordion>
//
// <nile-section>: `heading` (text), `open` (present = expanded), `icon` (HTML shown before the heading).
// Methods: open() / close() / toggle() on a section (`opened` property); on the accordion: expand(i), collapse(i),
// toggle(i), expandAll(), collapseAll(). `exclusive` on the accordion keeps one section open at a time. Events:
// `sectionchange` (detail: { section, index, open }), bubbling from the section. The heading is a real button:
// Enter/Space toggle it, Up/Down/Home/End move between headings of one accordion. Style with --nile-accordion-border,
// --nile-accordion-head-bg, --nile-accordion-hover, or .nile-section-head, .nile-section-body classes; the arrow is
// a ::before on the head.

import { style, emit } from "@nile/ui/base";

const CSS = `
:where(nile-accordion) { display: block; }
:where(nile-section) { display: block; border-bottom: 1px solid var(--nile-accordion-border, rgba(127, 127, 127, 0.35)); }
:where(nile-section) > .nile-section-head { display: flex; align-items: center; gap: 8px; width: 100%; box-sizing: border-box; text-align: left; padding: 10px 12px; border: 0; border-radius: 0; box-shadow: none;
  background: var(--nile-accordion-head-bg, transparent); color: inherit; font: inherit; font-weight: 600; cursor: pointer; }
:where(nile-section) > .nile-section-head:hover { background: var(--nile-accordion-hover, rgba(127, 127, 127, 0.12)); }
:where(nile-section) > .nile-section-head::before { content: "\\25B8"; display: inline-block; width: 1em; transition: transform 0.12s ease; }
:where(nile-section[open]) > .nile-section-head::before { transform: rotate(90deg); }
:where(nile-section) > .nile-section-body { display: none; padding: 4px 12px 12px 32px; }
:where(nile-section[open]) > .nile-section-body { display: block; }
`;

export function defineAccordion() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-section")) return;
  style("accordion", CSS);

  class NileSection extends HTMLElement {
    static get observedAttributes() {
      return ["open", "heading", "icon"];
    }
    connectedCallback() {
      if (!this._head) {
        const head = document.createElement("button");
        head.className = "nile-section-head";
        head.setAttribute("type", "button");
        const label = document.createElement("span");
        label.className = "nile-section-label";
        const body = document.createElement("div");
        body.className = "nile-section-body";
        Array.from(this.childNodes).forEach((n) => body.appendChild(n));
        head.appendChild(label);
        this.appendChild(head);
        this.appendChild(body);
        this._head = head;
        this._label = label;
        head.addEventListener("click", () => this.toggle());
        head.addEventListener("keydown", (e) => this._onKey(e));
      }
      this._sync();
    }
    attributeChangedCallback(attr, was, now) {
      if (!this._head) return;
      if (attr === "open") {
        if ((was === null) !== (now === null)) {
          this._sync();
          emit(this, "sectionchange", { section: this, index: this._index(), open: now !== null }, false);
        }
      } else this._sync();
    }
    _index() {
      const group = this.parentNode;
      return group && group.tagName === "NILE-ACCORDION" ? Array.from(group.children).filter((n) => n.tagName === "NILE-SECTION").indexOf(this) : 0;
    }
    _sync() {
      const text = this.getAttribute("heading") || "";
      const icon = this.getAttribute("icon");
      this._label.innerHTML = "";
      if (icon) {
        const span = document.createElement("span");
        span.innerHTML = icon;
        this._label.appendChild(span);
      }
      this._label.appendChild(document.createTextNode(text));
      this._head.setAttribute("aria-expanded", this.hasAttribute("open") ? "true" : "false");
    }
    get opened() {
      return this.hasAttribute("open");
    }
    open() {
      const group = this.parentNode;
      if (group && group.tagName === "NILE-ACCORDION" && group.hasAttribute("exclusive")) {
        Array.from(group.children).forEach((n) => {
          if (n !== this && n.tagName === "NILE-SECTION") n.removeAttribute("open");
        });
      }
      this.setAttribute("open", "");
      return this;
    }
    close() {
      this.removeAttribute("open");
      return this;
    }
    toggle() {
      return this.opened ? this.close() : this.open();
    }
    _onKey(e) {
      const group = this.parentNode;
      if (!group || group.tagName !== "NILE-ACCORDION") return;
      const heads = Array.from(group.children).filter((n) => n.tagName === "NILE-SECTION").map((n) => n._head);
      const at = heads.indexOf(this._head);
      let to = -1;
      if (e.key === "ArrowDown") to = (at + 1) % heads.length;
      else if (e.key === "ArrowUp") to = (at - 1 + heads.length) % heads.length;
      else if (e.key === "Home") to = 0;
      else if (e.key === "End") to = heads.length - 1;
      if (to < 0) return;
      e.preventDefault();
      heads[to].focus();
    }
  }

  class NileAccordion extends HTMLElement {
    get sections() {
      return Array.from(this.children).filter((n) => n.tagName === "NILE-SECTION");
    }
    expand(i) {
      const s = this.sections[i];
      if (s) s.open();
      return this;
    }
    collapse(i) {
      const s = this.sections[i];
      if (s) s.close();
      return this;
    }
    toggle(i) {
      const s = this.sections[i];
      if (s) s.toggle();
      return this;
    }
    expandAll() {
      if (!this.hasAttribute("exclusive")) this.sections.forEach((s) => s.setAttribute("open", ""));
      return this;
    }
    collapseAll() {
      this.sections.forEach((s) => s.removeAttribute("open"));
      return this;
    }
  }
  customElements.define("nile-section", NileSection);
  customElements.define("nile-accordion", NileAccordion);
}

defineAccordion();
