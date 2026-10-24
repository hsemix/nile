// @nile/ui/date: <nile-calendar>, <nile-date> and <nile-date-range>, restylable date pickers.
//
//   <nile-calendar value="2026-10-24"></nile-calendar>                       // a month you can click
//   <nile-calendar range start="2026-10-01" end="2026-10-15" months="2">     // pick a span
//   <nile-date value="2026-10-24" format="dd/MM/yyyy" min="2026-01-01"></nile-date>     // text field + popup calendar
//   <nile-date-range start="2026-10-01" end="2026-10-31"></nile-date-range>  // two dates with quick presets
//
// Values are ISO dates ("2026-10-24"); what the field shows is `format` (tokens yyyy yy MMMM MMM MM M dd d; default
// "yyyy-MM-dd"). Typed text is parsed with the same format when you press Enter or leave the field; if it doesn't
// parse the old value comes back. Common attributes: `min`, `max` (ISO), `week-start` (0 = Sunday, 1 = Monday, the
// default), `disabled`. A calendar also has `month` ("2026-10", what it shows), `range`, `months` (1 or 2 panes) and
// the `isDisabled(isoDate)` property (return true to grey a day out); a field has `placeholder`, `clearable`; a range
// field has `presets="false"` to hide the quick list and `preset-list` (a JS array of { label, range() -> [start, end] },
// set as the `presets` property) to replace it. Properties: value (calendar/date), start and end (range), months names
// via `calendar.monthNames` (12 strings) and `calendar.dayNames` (7, from Sunday). Events: `change` (detail: { value }
// or { start, end }), `monthchange` (detail: { month }). Keyboard in the calendar: arrows move a day / a week,
// PageUp/PageDown a month (Shift: a year), Home/End the start/end of the week, Enter or Space picks, Escape closes
// the popup. Style with --nile-date-accent, --nile-date-range, --nile-date-bg, --nile-date-border,
// --nile-date-color or the .nile-cal-day ([aria-selected], .nile-today, .nile-in-range, .nile-outside) classes.

import { style, emit, flagAttr, numberAttr, CHEVRON } from "@nile/ui/base";

const CSS = `
:where(nile-calendar) { display: inline-flex; gap: 12px; padding: 8px; box-sizing: border-box; background: var(--nile-date-bg, Canvas); color: var(--nile-date-color, CanvasText); user-select: none; }
:where(.nile-cal-pane) { display: flex; flex-direction: column; gap: 4px; width: 224px; }
:where(.nile-cal-head) { display: flex; align-items: center; height: 30px; }
:where(.nile-cal-title) { flex: 1; text-align: center; font-weight: 600; }
:where(nile-calendar) .nile-cal-nav { width: 28px; height: 28px; padding: 0; border: 0; border-radius: 4px; background: transparent; color: inherit; cursor: pointer; display: flex; align-items: center; justify-content: center; box-shadow: none; }
:where(nile-calendar) .nile-cal-nav:hover { background: rgba(127, 127, 127, 0.2); }
:where(nile-calendar) .nile-cal-nav svg { width: 10px; height: 6px; }
:where(nile-calendar) .nile-cal-nav[data-dir="-1"] svg { transform: rotate(90deg); }
:where(nile-calendar) .nile-cal-nav[data-dir="1"] svg { transform: rotate(-90deg); }
:where(.nile-cal-grid) { display: grid; grid-template-columns: repeat(7, 1fr); }
:where(.nile-cal-dow) { text-align: center; font-size: 11px; opacity: 0.6; padding: 4px 0; }
:where(nile-calendar) .nile-cal-day { height: 30px; margin: 1px 0; padding: 0; border: 0; border-radius: 4px; background: transparent; color: inherit; font: inherit; font-size: 13px; cursor: pointer; box-shadow: none; }
:where(nile-calendar) .nile-cal-day:hover { background: rgba(127, 127, 127, 0.2); }
:where(nile-calendar) .nile-cal-day.nile-outside { opacity: 0.4; }
:where(nile-calendar) .nile-cal-day.nile-today { box-shadow: inset 0 0 0 1px var(--nile-date-accent, #2563eb); }
:where(nile-calendar) .nile-cal-day.nile-in-range { background: var(--nile-date-range, rgba(37, 99, 235, 0.18)); border-radius: 0; }
:where(nile-calendar) .nile-cal-day[aria-selected="true"] { background: var(--nile-date-accent, #2563eb); color: #fff; }
:where(nile-calendar) .nile-cal-day.nile-focus { outline: 2px solid var(--nile-date-accent, #2563eb); outline-offset: -2px; }
:where(nile-calendar) .nile-cal-day[disabled] { opacity: 0.25; cursor: default; background: transparent; }
:where(nile-date), :where(nile-date-range) { display: inline-block; position: relative; vertical-align: middle; }
:where(.nile-date-box) { display: flex; align-items: stretch; border: 1px solid var(--nile-date-border, #94a3b8); border-radius: 4px; background: var(--nile-date-field, Field); color: var(--nile-date-color, FieldText); }
:where(nile-date:focus-within) > .nile-date-box, :where(nile-date-range:focus-within) > .nile-date-box { border-color: var(--nile-date-accent, #2563eb); box-shadow: 0 0 0 1px var(--nile-date-accent, #2563eb); }
:where(.nile-date-box) .nile-date-input { flex: 1; min-width: 0; width: 110px; border: 0; outline: none; background: transparent; color: inherit; font: inherit; padding: 6px 8px; box-shadow: none; }
:where(.nile-date-text) { flex: 1; min-width: 190px; padding: 6px 8px; display: flex; align-items: center; cursor: pointer; white-space: nowrap; }
:where(.nile-date-text:empty)::before { content: attr(data-placeholder); opacity: 0.55; }
:where(.nile-date-box) .nile-date-btn { flex: none; width: 28px; border: 0; background: transparent; color: inherit; padding: 0; cursor: pointer; opacity: 0.7; box-shadow: none; border-radius: 0; font-size: 14px; }
:where(.nile-date-pop) { position: fixed; z-index: 1300; display: none; box-sizing: border-box; border: 1px solid var(--nile-date-border, #94a3b8); border-radius: 6px; box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25); background: var(--nile-date-bg, Canvas); color: var(--nile-date-color, CanvasText); }
:where(.nile-date-pop.nile-open) { display: flex; }
:where(.nile-date-presets) { display: flex; flex-direction: column; padding: 6px; min-width: 120px; border-right: 1px solid var(--nile-date-border, #94a3b8); }
:where(.nile-date-presets) .nile-date-preset { padding: 6px 10px; text-align: left; border: 0; border-radius: 4px; background: transparent; color: inherit; font: inherit; cursor: pointer; box-shadow: none; white-space: nowrap; }
:where(.nile-date-presets) .nile-date-preset:hover { background: rgba(127, 127, 127, 0.2); }
:where(nile-date[disabled]), :where(nile-date-range[disabled]) { opacity: 0.55; pointer-events: none; }
`;

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

// ---- dates as ISO text ---------------------------------------------------------------------------------
const pad = (n, w) => String(n).padStart(w || 2, "0");
const iso = (y, m, d) => pad(y, 4) + "-" + pad(m) + "-" + pad(d);
/** { y, m (1-12), d } from "2026-10-24", or null when it isn't a real date. */
function split(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(text || ""));
  if (!m) return null;
  const y = Number(m[1]), mo = Number(m[2]), d = Number(m[3]);
  const t = new Date(y, mo - 1, d);
  return t.getFullYear() === y && t.getMonth() === mo - 1 && t.getDate() === d ? { y: y, m: mo, d: d } : null;
}
function fromDate(t) {
  return iso(t.getFullYear(), t.getMonth() + 1, t.getDate());
}
function today() {
  return fromDate(new Date());
}
function addDays(text, n) {
  const p = split(text);
  return fromDate(new Date(p.y, p.m - 1, p.d + n));
}
function addMonths(text, n) {
  const p = split(text);
  const t = new Date(p.y, p.m - 1 + n, 1);
  const last = new Date(t.getFullYear(), t.getMonth() + 1, 0).getDate();
  return iso(t.getFullYear(), t.getMonth() + 1, Math.min(p.d, last));
}
function weekday(text) {
  const p = split(text);
  return new Date(p.y, p.m - 1, p.d).getDay();
}
function monthOf(text) {
  return String(text).slice(0, 7);
}

/** Text for an ISO date in `format` (yyyy yy MMMM MMM MM M dd d). */
export function formatDate(text, format, names) {
  const p = split(text);
  if (!p) return "";
  const months = (names && names.months) || MONTHS;
  return String(format || "yyyy-MM-dd").replace(/yyyy|yy|MMMM|MMM|MM|M|dd|d/g, (t) => {
    switch (t) {
      case "yyyy": return pad(p.y, 4);
      case "yy": return pad(p.y % 100);
      case "MMMM": return months[p.m - 1];
      case "MMM": return months[p.m - 1].slice(0, 3);
      case "MM": return pad(p.m);
      case "M": return String(p.m);
      case "dd": return pad(p.d);
      default: return String(p.d);
    }
  });
}
/** The ISO date for text typed in `format`, or null. */
export function parseDate(input, format, names) {
  const months = (names && names.months) || MONTHS;
  const tokens = [];
  const re = String(format || "yyyy-MM-dd").replace(/[.*+?^${}()|[\]\\\/]/g, "\\$&").replace(/yyyy|yy|MMMM|MMM|MM|M|dd|d/g, (t) => {
    tokens.push(t);
    return t === "MMMM" || t === "MMM" ? "([A-Za-z]+)" : t === "yyyy" ? "(\\d{4})" : t === "yy" || t === "MM" || t === "dd" ? "(\\d{2})" : "(\\d{1,2})";
  });
  const m = new RegExp("^\\s*" + re + "\\s*$").exec(String(input));
  if (!m) return null;
  let y = NaN, mo = NaN, d = NaN;
  tokens.forEach((t, i) => {
    const v = m[i + 1];
    if (t === "yyyy") y = Number(v);
    else if (t === "yy") y = 2000 + Number(v);
    else if (t === "MMMM" || t === "MMM") mo = months.findIndex((n) => (t === "MMM" ? n.slice(0, 3) : n).toLowerCase() === v.toLowerCase()) + 1;
    else if (t === "MM" || t === "M") mo = Number(v);
    else d = Number(v);
  });
  if (Number.isNaN(y) || Number.isNaN(mo) || Number.isNaN(d) || mo < 1) return null;
  const text = iso(y, mo, d);
  return split(text) ? text : null;
}

export function defineDate() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-calendar")) return;
  style("date", CSS);

  /** Place a fixed popup under `anchor`, or above it when there is no room, and keep it on screen. */
  function place(pop, anchor) {
    const r = anchor.getBoundingClientRect();
    const h = pop.offsetHeight || 260;
    const room = (window.innerHeight || 600) - r.bottom;
    pop.style.left = Math.max(0, Math.min(r.left, (window.innerWidth || 800) - (pop.offsetWidth || 240))) + "px";
    if (room < h && r.top > room) {
      pop.style.top = "";
      pop.style.bottom = (window.innerHeight || 600) - r.top + "px";
    } else {
      pop.style.bottom = "";
      pop.style.top = r.bottom + "px";
    }
  }

  class NileCalendar extends HTMLElement {
    static get observedAttributes() {
      return ["value", "start", "end", "month", "min", "max", "week-start", "range", "months"];
    }
    get _st() {
      return this._state || (this._state = { built: false, value: null, start: null, end: null, month: null, focus: null, hover: null, picking: false, disabled: null, names: { months: MONTHS, days: DAYS } });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      this.setAttribute("role", "application");
      if (!this.hasAttribute("aria-label")) this.setAttribute("aria-label", "Calendar");
      if (st.value === null && this.hasAttribute("value")) st.value = split(this.getAttribute("value")) ? this.getAttribute("value") : null;
      if (st.start === null && this.hasAttribute("start")) st.start = split(this.getAttribute("start")) ? this.getAttribute("start") : null;
      if (st.end === null && this.hasAttribute("end")) st.end = split(this.getAttribute("end")) ? this.getAttribute("end") : null;
      if (!st.month) st.month = this.hasAttribute("month") && /^\d{4}-\d{2}$/.test(this.getAttribute("month")) ? this.getAttribute("month") : monthOf(st.value || st.start || today());
      st.focus = st.value || st.start || (monthOf(today()) === st.month ? today() : st.month + "-01");
      this.addEventListener("click", (e) => this._click(e));
      this.addEventListener("keydown", (e) => this._onKey(e));
      this.addEventListener("mouseover", (e) => this._over(e));
      this._paint();
    }
    attributeChangedCallback(attr, was, now) {
      const st = this._st;
      if (!st.built || was === now) return;
      if (attr === "value") st.value = split(now) ? now : null;
      else if (attr === "start") st.start = split(now) ? now : null;
      else if (attr === "end") st.end = split(now) ? now : null;
      else if (attr === "month" && /^\d{4}-\d{2}$/.test(now || "")) st.month = now;
      this._paint();
    }
    get monthNames() {
      return this._st.names.months;
    }
    set monthNames(list) {
      if (Array.isArray(list) && list.length === 12) this._st.names.months = list.map(String);
      this._paint();
    }
    get dayNames() {
      return this._st.names.days;
    }
    set dayNames(list) {
      if (Array.isArray(list) && list.length === 7) this._st.names.days = list.map(String);
      this._paint();
    }
    get isDisabled() {
      return this._st.disabled;
    }
    set isDisabled(fn) {
      this._st.disabled = typeof fn === "function" ? fn : null;
      this._paint();
    }
    get value() {
      return this._st.value;
    }
    set value(v) {
      const st = this._st;
      st.value = split(v) ? String(v) : null;
      if (st.value) {
        st.month = monthOf(st.value);
        st.focus = st.value;
      }
      this._paint();
    }
    get start() {
      return this._st.start;
    }
    get end() {
      return this._st.end;
    }
    /** Set the selected span. */
    setRange(start, end) {
      const st = this._st;
      st.start = split(start) ? String(start) : null;
      st.end = split(end) ? String(end) : null;
      if (st.start && st.end && st.end < st.start) [st.start, st.end] = [st.end, st.start];
      st.picking = false;
      if (st.start) {
        st.month = monthOf(st.start);
        st.focus = st.start;
      }
      this._paint();
      return this;
    }
    /** Show a month ("2026-10") without changing the selection. */
    show(month) {
      const st = this._st;
      if (!/^\d{4}-\d{2}$/.test(String(month))) return this;
      st.month = String(month);
      this._paint();
      emit(this, "monthchange", { month: st.month }, false);
      return this;
    }
    _isRange() {
      return this.hasAttribute("range");
    }
    _blocked(day) {
      const min = this.getAttribute("min"), max = this.getAttribute("max");
      if (min && day < min) return true;
      if (max && day > max) return true;
      return !!(this._st.disabled && this._st.disabled(day));
    }
    _weekStart() {
      const w = numberAttr(this, "week-start", 1);
      return w >= 0 && w <= 6 ? Math.floor(w) : 1;
    }
    _paint() {
      const st = this._st;
      if (!st.built) return;
      this.innerHTML = "";
      const count = Math.max(1, Math.min(2, numberAttr(this, "months", 1)));
      for (let i = 0; i < count; i++) this.appendChild(this._pane(addMonths(st.month + "-01", i), i, count));
    }
    _pane(first, index, count) {
      const st = this._st;
      const p = split(first);
      const pane = document.createElement("div");
      pane.className = "nile-cal-pane";
      const head = document.createElement("div");
      head.className = "nile-cal-head";
      const nav = (dir) => {
        const b = document.createElement("button");
        b.className = "nile-cal-nav";
        b.setAttribute("type", "button");
        b.setAttribute("tabindex", "-1");
        b.setAttribute("data-dir", String(dir));
        b.setAttribute("aria-label", dir < 0 ? "Previous month" : "Next month");
        b.innerHTML = CHEVRON;
        return b;
      };
      if (index === 0) head.appendChild(nav(-1));
      else head.appendChild(document.createElement("span")).className = "nile-cal-nav";
      const title = document.createElement("div");
      title.className = "nile-cal-title";
      title.setAttribute("aria-live", "polite");
      title.textContent = st.names.months[p.m - 1] + " " + p.y;
      head.appendChild(title);
      if (index === count - 1) head.appendChild(nav(1));
      else head.appendChild(document.createElement("span")).className = "nile-cal-nav";
      pane.appendChild(head);
      const grid = document.createElement("div");
      grid.className = "nile-cal-grid";
      grid.setAttribute("role", "grid");
      const ws = this._weekStart();
      for (let i = 0; i < 7; i++) {
        const dow = document.createElement("div");
        dow.className = "nile-cal-dow";
        dow.textContent = st.names.days[(ws + i) % 7].slice(0, 2);
        grid.appendChild(dow);
      }
      const lead = (weekday(first) - ws + 7) % 7;
      const todayIso = today();
      const lo = this._isRange() ? this._span()[0] : null, hi = this._isRange() ? this._span()[1] : null;
      for (let i = 0; i < 42; i++) {
        const day = addDays(first, i - lead);
        const outside = monthOf(day) !== monthOf(first);
        const b = document.createElement("button");
        b.className = "nile-cal-day";
        b.setAttribute("type", "button");
        b.setAttribute("data-date", day);
        b.setAttribute("tabindex", "-1");
        b.textContent = String(Number(day.slice(8)));
        if (outside) {
          b.classList.add("nile-outside");
          // Neighbouring months stay out of the way in a two-pane calendar.
          if (count > 1) {
            b.style.visibility = "hidden";
            b.setAttribute("disabled", "");
            grid.appendChild(b);
            continue;
          }
        }
        if (day === todayIso) b.classList.add("nile-today");
        if (this._blocked(day)) b.setAttribute("disabled", "");
        let selected = false;
        if (this._isRange()) {
          selected = day === lo || day === hi;
          if (lo && hi && day > lo && day < hi) b.classList.add("nile-in-range");
        } else selected = day === st.value;
        b.setAttribute("aria-selected", selected ? "true" : "false");
        if (day === st.focus) b.classList.add("nile-focus");
        grid.appendChild(b);
      }
      pane.appendChild(grid);
      return pane;
    }
    /** The span being shown: the picked one, or the one the pointer is previewing while the end is still open. */
    _span() {
      const st = this._st;
      let a = st.start, b = st.end;
      if (st.picking && st.start) {
        b = st.hover && !this._blocked(st.hover) ? st.hover : st.start;
        a = st.start;
      }
      if (a && b && b < a) [a, b] = [b, a];
      return [a, b];
    }
    _click(e) {
      const st = this._st;
      const nav = e.target.closest && e.target.closest(".nile-cal-nav[data-dir]");
      if (nav) {
        this.show(monthOf(addMonths(st.month + "-01", Number(nav.getAttribute("data-dir")))));
        return;
      }
      const btn = e.target.closest && e.target.closest(".nile-cal-day");
      if (btn && !btn.disabled) this._pick(btn.getAttribute("data-date"));
    }
    _over(e) {
      const st = this._st;
      if (!st.picking) return;
      const btn = e.target.closest && e.target.closest(".nile-cal-day");
      const day = btn ? btn.getAttribute("data-date") : null;
      if (day && day !== st.hover) {
        st.hover = day;
        this._mark();
      }
    }
    /** Repaint the range highlight in place (no rebuild while the pointer moves). */
    _mark() {
      const [lo, hi] = this._span();
      Array.from(this.querySelectorAll(".nile-cal-day")).forEach((b) => {
        const day = b.getAttribute("data-date");
        if (lo && hi && day > lo && day < hi) b.classList.add("nile-in-range");
        else b.classList.remove("nile-in-range");
        b.setAttribute("aria-selected", day === lo || day === hi ? "true" : "false");
      });
    }
    _pick(day) {
      const st = this._st;
      if (this._blocked(day)) return;
      st.focus = day;
      if (this._isRange()) {
        if (!st.picking) {
          st.start = day;
          st.end = null;
          st.picking = true;
          st.hover = null;
          this._paint();
        } else {
          st.picking = false;
          if (day < st.start) {
            st.end = st.start;
            st.start = day;
          } else st.end = day;
          this._paint();
          emit(this, "change", { start: st.start, end: st.end }, false);
        }
        return;
      }
      st.value = day;
      if (monthOf(day) !== st.month && numberAttr(this, "months", 1) < 2) st.month = monthOf(day);
      this._paint();
      emit(this, "change", { value: day }, false);
    }
    _onKey(e) {
      const st = this._st;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      let at = st.focus || today();
      const ws = this._weekStart();
      switch (e.key) {
        case "ArrowLeft": at = addDays(at, -1); break;
        case "ArrowRight": at = addDays(at, 1); break;
        case "ArrowUp": at = addDays(at, -7); break;
        case "ArrowDown": at = addDays(at, 7); break;
        case "PageUp": at = addMonths(at, e.shiftKey ? -12 : -1); break;
        case "PageDown": at = addMonths(at, e.shiftKey ? 12 : 1); break;
        case "Home": at = addDays(at, -((weekday(at) - ws + 7) % 7)); break;
        case "End": at = addDays(at, 6 - ((weekday(at) - ws + 7) % 7)); break;
        case "Enter":
        case " ":
          e.preventDefault();
          this._pick(at);
          return;
        default: return;
      }
      e.preventDefault();
      st.focus = at;
      st.hover = st.picking ? at : null;
      const shown = numberAttr(this, "months", 1) > 1 ? [st.month, monthOf(addMonths(st.month + "-01", 1))] : [st.month];
      if (!shown.includes(monthOf(at))) {
        st.month = monthOf(at);
        emit(this, "monthchange", { month: st.month }, false);
      }
      this._paint();
      const cell = this.querySelector('.nile-cal-day[data-date="' + at + '"]');
      if (cell && cell.focus) cell.focus();
    }
    focus() {
      const st = this._st;
      const cell = this.querySelector('.nile-cal-day[data-date="' + st.focus + '"]') || this.querySelector(".nile-cal-day:not([disabled])");
      if (cell && cell.focus) cell.focus();
    }
  }

  /** A popup of `calendar` under a field; closes on Escape, an outside press or a pick. */
  function popup(host, content) {
    const pop = document.createElement("div");
    pop.className = "nile-date-pop";
    pop.appendChild(content);
    host.appendChild(pop);
    const outside = (e) => {
      if (!host.contains(e.target)) close();
    };
    const key = (e) => {
      if (e.key === "Escape" && pop.classList.contains("nile-open")) {
        e.stopPropagation();
        close();
        if (host._focusField) host._focusField();
      }
    };
    function open() {
      if (host.hasAttribute("disabled")) return;
      pop.classList.add("nile-open");
      place(pop, host._box);
      document.addEventListener("mousedown", outside, true);
      document.addEventListener("keydown", key, true);
    }
    function close() {
      pop.classList.remove("nile-open");
      document.removeEventListener("mousedown", outside, true);
      document.removeEventListener("keydown", key, true);
    }
    return { pop: pop, open: open, close: close, isOpen: () => pop.classList.contains("nile-open") };
  }

  class NileDate extends HTMLElement {
    static get observedAttributes() {
      return ["value", "min", "max", "format", "placeholder", "week-start"];
    }
    get _st() {
      return this._state || (this._state = { built: false, value: null });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      if (st.value === null && this.hasAttribute("value") && split(this.getAttribute("value"))) st.value = this.getAttribute("value");
      this._box = document.createElement("div");
      this._box.className = "nile-date-box";
      this._input = document.createElement("input");
      this._input.className = "nile-date-input";
      this._input.setAttribute("type", "text");
      this._input.setAttribute("autocomplete", "off");
      this._btn = document.createElement("button");
      this._btn.className = "nile-date-btn";
      this._btn.setAttribute("type", "button");
      this._btn.setAttribute("tabindex", "-1");
      this._btn.setAttribute("aria-label", "Choose date");
      this._btn.innerHTML = CHEVRON;
      this._box.appendChild(this._input);
      this._box.appendChild(this._btn);
      this.appendChild(this._box);
      this._cal = document.createElement("nile-calendar");
      this._pop = popup(this, this._cal);
      this._focusField = () => this._input.focus();
      this._cal.addEventListener("change", (e) => {
        e.stopPropagation();
        this._set(e.detail.value, true);
        this._pop.close();
        this._input.focus();
      });
      this._btn.addEventListener("mousedown", (e) => e.preventDefault());
      this._btn.addEventListener("click", () => this._toggle());
      this._input.addEventListener("keydown", (e) => {
        if (e.key === "Enter") {
          this._commitText();
          e.preventDefault();
        } else if (e.key === "ArrowDown" && (e.altKey || !this._pop.isOpen())) {
          e.preventDefault();
          this._toggle(true);
        } else if (e.key === "Escape") this._sync();
      });
      this._input.addEventListener("blur", () => setTimeout(() => this._st.built && !this._pop.isOpen() && this._commitText(), 0));
      this._cal.addEventListener("keydown", (e) => {
        if (e.key === "Tab") this._pop.close();
      });
      this._applyAttrs();
      this._sync();
    }
    attributeChangedCallback(attr, was, now) {
      const st = this._st;
      if (!st.built || was === now) return;
      if (attr === "value") st.value = split(now) ? now : null;
      this._applyAttrs();
      this._sync();
    }
    _applyAttrs() {
      ["min", "max", "week-start"].forEach((a) => {
        if (this.hasAttribute(a)) this._cal.setAttribute(a, this.getAttribute(a));
        else this._cal.removeAttribute(a);
      });
      this._input.setAttribute("placeholder", this.getAttribute("placeholder") || this._format().toLowerCase());
    }
    _format() {
      return this.getAttribute("format") || "yyyy-MM-dd";
    }
    get value() {
      return this._st.value;
    }
    set value(v) {
      this._set(v, false);
    }
    get calendar() {
      return this._cal;
    }
    _sync() {
      const st = this._st;
      if (!st.built) return;
      this._input.value = st.value ? formatDate(st.value, this._format(), this._cal._st.names) : "";
      if (st.value) this._cal.value = st.value;
      this.removeAttribute("invalid");
    }
    _set(v, notify) {
      const st = this._st;
      const next = v === null || v === undefined || v === "" ? null : split(v) ? String(v) : st.value;
      const min = this.getAttribute("min"), max = this.getAttribute("max");
      if (next && ((min && next < min) || (max && next > max))) {
        this.setAttribute("invalid", "");
        this._sync();
        this.setAttribute("invalid", "");
        return;
      }
      const changed = next !== st.value;
      st.value = next;
      this._sync();
      if (changed && notify) emit(this, "change", { value: next }, false);
    }
    _commitText() {
      const text = this._input.value.trim();
      if (text === "") {
        if (this._st.value !== null) this._set(null, true);
        return;
      }
      const parsed = parseDate(text, this._format(), this._cal._st.names);
      if (parsed) this._set(parsed, true);
      else {
        this._sync();
        this.setAttribute("invalid", "");
      }
    }
    _toggle(open) {
      if (this._pop.isOpen() && !open) this._pop.close();
      else {
        this._cal.value = this._st.value;
        this._pop.open();
        this._cal.focus();
      }
    }
  }

  const iso0 = () => today();
  const PRESETS = [
    { label: "Today", range: () => [iso0(), iso0()] },
    { label: "Yesterday", range: () => [addDays(iso0(), -1), addDays(iso0(), -1)] },
    { label: "Last 7 days", range: () => [addDays(iso0(), -6), iso0()] },
    { label: "Last 30 days", range: () => [addDays(iso0(), -29), iso0()] },
    { label: "This month", range: () => [monthOf(iso0()) + "-01", addDays(addMonths(monthOf(iso0()) + "-01", 1), -1)] },
    { label: "Last month", range: () => [addMonths(monthOf(iso0()) + "-01", -1), addDays(monthOf(iso0()) + "-01", -1)] },
    { label: "This year", range: () => [iso0().slice(0, 4) + "-01-01", iso0().slice(0, 4) + "-12-31"] },
  ];

  class NileDateRange extends HTMLElement {
    static get observedAttributes() {
      return ["start", "end", "min", "max", "format", "placeholder", "week-start", "presets"];
    }
    get _st() {
      return this._state || (this._state = { built: false, start: null, end: null, presets: null });
    }
    connectedCallback() {
      const st = this._st;
      if (st.built) return;
      st.built = true;
      if (st.start === null && split(this.getAttribute("start"))) st.start = this.getAttribute("start");
      if (st.end === null && split(this.getAttribute("end"))) st.end = this.getAttribute("end");
      this._box = document.createElement("div");
      this._box.className = "nile-date-box";
      this._box.setAttribute("tabindex", "0");
      this._box.setAttribute("role", "button");
      this._text = document.createElement("div");
      this._text.className = "nile-date-text";
      this._btn = document.createElement("span");
      this._btn.className = "nile-date-btn";
      this._btn.style.display = "flex";
      this._btn.style.alignItems = "center";
      this._btn.style.justifyContent = "center";
      this._btn.innerHTML = CHEVRON;
      this._box.appendChild(this._text);
      this._box.appendChild(this._btn);
      this.appendChild(this._box);
      this._cal = document.createElement("nile-calendar");
      this._cal.setAttribute("range", "");
      this._cal.setAttribute("months", "2");
      const wrap = document.createElement("div");
      wrap.style.display = "flex";
      this._side = document.createElement("div");
      this._side.className = "nile-date-presets";
      wrap.appendChild(this._side);
      wrap.appendChild(this._cal);
      this._pop = popup(this, wrap);
      this._focusField = () => this._box.focus();
      this._cal.addEventListener("change", (e) => {
        e.stopPropagation();
        this._set(e.detail.start, e.detail.end, true);
        this._pop.close();
        this._box.focus();
      });
      this._box.addEventListener("click", () => this._toggle());
      this._box.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown") {
          e.preventDefault();
          this._toggle(true);
        }
      });
      this._side.addEventListener("click", (e) => {
        const b = e.target.closest && e.target.closest(".nile-date-preset");
        if (!b) return;
        const [a, z] = this._presets()[Number(b.getAttribute("data-index"))].range();
        this._set(a, z, true);
        this._pop.close();
        this._box.focus();
      });
      this._cal.addEventListener("keydown", (e) => {
        if (e.key === "Tab") this._pop.close();
      });
      this._applyAttrs();
      this._sync();
    }
    attributeChangedCallback(attr, was, now) {
      const st = this._st;
      if (!st.built || was === now) return;
      if (attr === "start") st.start = split(now) ? now : null;
      if (attr === "end") st.end = split(now) ? now : null;
      this._applyAttrs();
      this._sync();
    }
    _presets() {
      return this._st.presets || PRESETS;
    }
    get presets() {
      return this._st.presets;
    }
    set presets(list) {
      this._st.presets = Array.isArray(list) ? list : null;
      if (this._st.built) this._applyAttrs();
    }
    _applyAttrs() {
      ["min", "max", "week-start"].forEach((a) => {
        if (this.hasAttribute(a)) this._cal.setAttribute(a, this.getAttribute(a));
        else this._cal.removeAttribute(a);
      });
      this._side.innerHTML = "";
      const show = flagAttr(this, "presets", true);
      this._side.style.display = show ? "" : "none";
      if (show) {
        this._presets().forEach((p, i) => {
          const b = document.createElement("button");
          b.className = "nile-date-preset";
          b.setAttribute("type", "button");
          b.setAttribute("data-index", String(i));
          b.textContent = p.label;
          this._side.appendChild(b);
        });
      }
    }
    _format() {
      return this.getAttribute("format") || "yyyy-MM-dd";
    }
    get start() {
      return this._st.start;
    }
    get end() {
      return this._st.end;
    }
    get value() {
      return { start: this._st.start, end: this._st.end };
    }
    set value(v) {
      if (v && typeof v === "object") this._set(v.start, v.end, false);
    }
    /** Set both ends (ISO dates); the earlier one becomes the start. */
    setRange(start, end) {
      this._set(start, end, false);
      return this;
    }
    get calendar() {
      return this._cal;
    }
    _sync() {
      const st = this._st;
      if (!st.built) return;
      const names = this._cal._st.names;
      this._text.setAttribute("data-placeholder", this.getAttribute("placeholder") || "Select dates");
      this._text.textContent = st.start && st.end ? (st.start === st.end ? formatDate(st.start, this._format(), names) : formatDate(st.start, this._format(), names) + " – " + formatDate(st.end, this._format(), names)) : "";
      this._cal.setRange(st.start, st.end);
    }
    _set(a, b, notify) {
      const st = this._st;
      let start = split(a) ? String(a) : null, end = split(b) ? String(b) : null;
      if (start && end && end < start) [start, end] = [end, start];
      const min = this.getAttribute("min"), max = this.getAttribute("max");
      if (start && min && start < min) start = min;
      if (end && max && end > max) end = max;
      const changed = start !== st.start || end !== st.end;
      st.start = start;
      st.end = end;
      this._sync();
      if (changed && notify) emit(this, "change", { start: start, end: end }, false);
    }
    _toggle(open) {
      if (this._pop.isOpen() && !open) this._pop.close();
      else {
        this._pop.open();
        this._cal.focus();
      }
    }
  }

  customElements.define("nile-calendar", NileCalendar);
  customElements.define("nile-date", NileDate);
  customElements.define("nile-date-range", NileDateRange);
}

defineDate();
