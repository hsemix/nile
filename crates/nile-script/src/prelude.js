// The web-facing DOM, built on a handful of native functions (`__nile`).
// Nodes are thin wrappers around arena ids, cached so `a === b` holds.
"use strict";
(function (g) {
  const N = g.__nile;
  delete g.__nile;

  const report = (e) => N.log("error", "Uncaught " + (e && e.stack ? e.stack : e));
  const kebab = (n) => String(n).replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
  const idOf = (n) => {
    if (n instanceof Node) return n._id;
    throw new TypeError("parameter is not of type 'Node'");
  };

  // ---- events ------------------------------------------------------------
  const listeners = new WeakMap();

  class EventTarget {
    addEventListener(type, fn, opts) {
      if (typeof fn !== "function" && !(fn && typeof fn.handleEvent === "function")) return;
      const capture = typeof opts === "boolean" ? opts : !!(opts && opts.capture);
      const once = !!(opts && typeof opts === "object" && opts.once);
      let m = listeners.get(this);
      if (!m) listeners.set(this, (m = Object.create(null)));
      const arr = m[type] || (m[type] = []);
      if (arr.some((r) => r.fn === fn && r.capture === capture)) return;
      arr.push({ fn, capture, once, removed: false });
    }
    removeEventListener(type, fn, opts) {
      const capture = typeof opts === "boolean" ? opts : !!(opts && opts.capture);
      const arr = (listeners.get(this) || {})[type];
      if (!arr) return;
      const i = arr.findIndex((r) => r.fn === fn && r.capture === capture);
      if (i >= 0) {
        arr[i].removed = true;
        arr.splice(i, 1);
      }
    }
    dispatchEvent(ev) {
      return dispatch(this, ev);
    }
  }

  class Event {
    constructor(type, init = {}) {
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.eventPhase = 0;
      this.isTrusted = false;
      this.timeStamp = N.now();
      this._stop = false;
      this._stopNow = false;
    }
    stopPropagation() { this._stop = true; }
    stopImmediatePropagation() { this._stop = this._stopNow = true; }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
  }
  class UIEvent extends Event {}
  class MouseEvent extends UIEvent {
    constructor(type, init = {}) {
      super(type, init);
      this.clientX = this.x = this.pageX = this.screenX = init.clientX || 0;
      this.clientY = this.y = this.pageY = this.screenY = init.clientY || 0;
      this.button = init.button || 0;
      this.buttons = init.buttons || 0;
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
    }
  }
  class KeyboardEvent extends UIEvent {
    constructor(type, init = {}) {
      super(type, init);
      this.key = init.key || "";
      this.code = init.code || "";
      this.ctrlKey = !!init.ctrlKey;
      this.shiftKey = !!init.shiftKey;
      this.altKey = !!init.altKey;
      this.metaKey = !!init.metaKey;
    }
  }
  class FocusEvent extends UIEvent {}
  class CustomEvent extends Event {
    constructor(type, init = {}) {
      super(type, init);
      this.detail = init.detail === undefined ? null : init.detail;
    }
  }

  const parentOf = (t) => {
    if (t === g) return null;
    if (t instanceof Document) return g;
    return t.parentNode;
  };

  function invoke(t, ev, phase) {
    ev.currentTarget = t;
    if (phase !== "capture") {
      let h = t["on" + ev.type];
      if (typeof h !== "function" && t instanceof Element) {
        const src = t.getAttribute("on" + ev.type);
        if (src !== null) {
          t._attr = t._attr || {};
          if (!t._attr[ev.type] || t._attr[ev.type].src !== src) {
            try { t._attr[ev.type] = { src, fn: new Function("event", src) }; }
            catch (e) { report(e); t._attr[ev.type] = { src, fn: null }; }
          }
          h = t._attr[ev.type].fn;
        }
      }
      if (typeof h === "function") {
        try { if (h.call(t, ev) === false) ev.preventDefault(); } catch (e) { report(e); }
      }
    }
    const arr = (listeners.get(t) || {})[ev.type];
    if (!arr) return;
    for (const rec of arr.slice()) {
      if (rec.removed) continue;
      if (phase === "capture" && !rec.capture) continue;
      if (phase === "bubble" && rec.capture) continue;
      if (rec.once) t.removeEventListener(ev.type, rec.fn, rec.capture);
      try {
        if (typeof rec.fn === "function") rec.fn.call(t, ev);
        else rec.fn.handleEvent(ev);
      } catch (e) { report(e); }
      if (ev._stopNow) break;
    }
  }

  // Returns false if a listener called preventDefault().
  function dispatch(target, ev) {
    ev.target = target;
    const path = [];
    for (let t = parentOf(target); t; t = parentOf(t)) path.push(t);
    ev.eventPhase = 1;
    for (let i = path.length - 1; i >= 0 && !ev._stop; i--) invoke(path[i], ev, "capture");
    if (!ev._stop) {
      ev.eventPhase = 2;
      invoke(target, ev, "target");
    }
    if (ev.bubbles) {
      ev.eventPhase = 3;
      for (let i = 0; i < path.length && !ev._stop; i++) invoke(path[i], ev, "bubble");
    }
    ev.eventPhase = 0;
    ev.currentTarget = null;
    return !ev.defaultPrevented;
  }

  // ---- nodes -------------------------------------------------------------
  const cache = new Map();
  function wrap(id) {
    if (id < 0) return null;
    let w = cache.get(id);
    if (!w) {
      const t = N.nodeType(id);
      w = t === 1 ? new Element(id) : t === 3 ? new Text(id) : documentObj;
      cache.set(id, w);
    }
    return w;
  }

  class Node extends EventTarget {
    constructor(id) { super(); this._id = id; }
    get nodeType() { return N.nodeType(this._id); }
    get parentNode() { return wrap(N.parent(this._id)); }
    get parentElement() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; }
    get childNodes() { return N.childNodes(this._id).map(wrap); }
    get firstChild() { return this.childNodes[0] || null; }
    get lastChild() { const c = this.childNodes; return c[c.length - 1] || null; }
    get nextSibling() { return this._sibling(1); }
    get previousSibling() { return this._sibling(-1); }
    _sibling(d) {
      const p = this.parentNode;
      if (!p) return null;
      const c = p.childNodes;
      return c[c.indexOf(this) + d] || null;
    }
    get textContent() { return N.text(this._id); }
    set textContent(v) { N.setText(this._id, String(v)); }
    get isConnected() {
      for (let n = this; n; n = n.parentNode) if (n === documentObj) return true;
      return false;
    }
    appendChild(c) { N.append(this._id, idOf(c)); return c; }
    insertBefore(c, ref) { N.insertBefore(this._id, idOf(c), ref ? idOf(ref) : -1); return c; }
    removeChild(c) {
      if (c.parentNode !== this) throw new Error("NotFoundError: the node is not a child of this node");
      N.remove(c._id);
      return c;
    }
    remove() { N.remove(this._id); }
    contains(o) { for (let n = o; n; n = n.parentNode) if (n === this) return true; return false; }
  }

  class Text extends Node {
    get nodeName() { return "#text"; }
    get data() { return N.text(this._id); }
    set data(v) { N.setText(this._id, String(v)); }
    get nodeValue() { return this.data; }
    set nodeValue(v) { this.data = v; }
  }

  class ClassList {
    constructor(el) { this.el = el; }
    _tokens() { return (this.el.getAttribute("class") || "").split(/\s+/).filter(Boolean); }
    _save(t) { this.el.setAttribute("class", t.join(" ")); }
    get length() { return this._tokens().length; }
    item(i) { return this._tokens()[i] ?? null; }
    contains(c) { return this._tokens().includes(c); }
    add(...cs) { const t = this._tokens(); for (const c of cs) if (!t.includes(c)) t.push(c); this._save(t); }
    remove(...cs) { this._save(this._tokens().filter((c) => !cs.includes(c))); }
    toggle(c, force) {
      const has = this.contains(c);
      const want = force === undefined ? !has : !!force;
      if (want && !has) this.add(c);
      else if (!want && has) this.remove(c);
      return want;
    }
    toString() { return this._tokens().join(" "); }
    [Symbol.iterator]() { return this._tokens()[Symbol.iterator](); }
  }

  const parseDecls = (s) =>
    s.split(";").map((x) => x.trim()).filter(Boolean)
      .map((x) => { const i = x.indexOf(":"); return i < 0 ? null : [x.slice(0, i).trim().toLowerCase(), x.slice(i + 1).trim()]; })
      .filter(Boolean);

  function styleOf(el) {
    if (el._style) return el._style;
    const read = () => parseDecls(el.getAttribute("style") || "");
    const write = (d) => el.setAttribute("style", d.map(([k, v]) => k + ": " + v).join("; "));
    const api = {
      getPropertyValue(n) { const d = read().find((x) => x[0] === kebab(n)); return d ? d[1] : ""; },
      setProperty(n, v) {
        n = kebab(n);
        const d = read().filter((x) => x[0] !== n);
        if (v !== "" && v != null) d.push([n, String(v)]);
        write(d);
      },
      removeProperty(n) { const old = api.getPropertyValue(n); api.setProperty(n, ""); return old; },
      get cssText() { return el.getAttribute("style") || ""; },
      set cssText(v) { el.setAttribute("style", String(v)); },
      get length() { return read().length; },
      item(i) { return (read()[i] || [""])[0]; },
    };
    return (el._style = new Proxy(api, {
      get(t, k) { return typeof k === "symbol" ? undefined : k in t ? t[k] : t.getPropertyValue(k); },
      set(t, k, v) { if (k === "cssText") t.cssText = v; else t.setProperty(k, v); return true; },
    }));
  }

  class Element extends Node {
    get nodeName() { return this.tagName; }
    get tagName() { return N.tag(this._id).toUpperCase(); }
    get localName() { return N.tag(this._id); }
    get id() { return this.getAttribute("id") || ""; }
    set id(v) { this.setAttribute("id", v); }
    get className() { return this.getAttribute("class") || ""; }
    set className(v) { this.setAttribute("class", v); }
    get classList() { return (this._classList = this._classList || new ClassList(this)); }
    get style() { return styleOf(this); }
    get dataset() {
      return (this._dataset = this._dataset || new Proxy({}, {
        get: (_, k) => (typeof k === "symbol" ? undefined : this.getAttribute("data-" + kebab(k)) ?? undefined),
        set: (_, k, v) => { this.setAttribute("data-" + kebab(k), v); return true; },
        deleteProperty: (_, k) => { this.removeAttribute("data-" + kebab(k)); return true; },
        has: (_, k) => this.hasAttribute("data-" + kebab(k)),
      }));
    }
    getAttribute(n) { return N.getAttr(this._id, String(n)) ?? null; } // natives return undefined for None
    setAttribute(n, v) { N.setAttr(this._id, String(n), String(v)); }
    removeAttribute(n) { N.removeAttr(this._id, String(n)); }
    hasAttribute(n) { return this.getAttribute(n) !== null; }
    getAttributeNames() { return N.attrNames(this._id); }
    get children() { return this.childNodes.filter((n) => n.nodeType === 1); }
    get childElementCount() { return this.children.length; }
    get firstElementChild() { return this.children[0] || null; }
    get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }
    get nextElementSibling() { for (let n = this.nextSibling; n; n = n.nextSibling) if (n.nodeType === 1) return n; return null; }
    get previousElementSibling() { for (let n = this.previousSibling; n; n = n.previousSibling) if (n.nodeType === 1) return n; return null; }
    get innerHTML() { return N.innerHTML(this._id); }
    set innerHTML(v) { N.setInnerHTML(this._id, String(v)); }
    append(...items) { for (const i of items) this.appendChild(typeof i === "object" ? i : documentObj.createTextNode(String(i))); }
    querySelector(s) { const r = N.select(this._id, String(s)); return r.length ? wrap(r[0]) : null; }
    querySelectorAll(s) { return N.select(this._id, String(s)).map(wrap); }
    matches(s) { return N.matches(this._id, String(s)); }
    closest(s) { for (let n = this; n && n.nodeType === 1; n = n.parentNode) if (n.matches(s)) return n; return null; }
    click() { dispatch(this, new MouseEvent("click", { bubbles: true, cancelable: true })); }

    // Layout queries. Rects are in viewport coordinates, like browsers'.
    getBoundingClientRect() {
      const [x, y, w, h] = N.rect(this._id) || [0, 0, 0, 0];
      return { x, y, width: w, height: h, left: x, top: y, right: x + w, bottom: y + h, toJSON() { return { ...this }; } };
    }
    get offsetWidth() { return Math.round(this.getBoundingClientRect().width); }
    get offsetHeight() { return Math.round(this.getBoundingClientRect().height); }
    // TODO: offsetParent is approximated by the parent element.
    get offsetLeft() { const p = this.parentElement; return Math.round(this.getBoundingClientRect().left - (p ? p.getBoundingClientRect().left : 0)); }
    get offsetTop() { const p = this.parentElement; return Math.round(this.getBoundingClientRect().top - (p ? p.getBoundingClientRect().top : 0)); }
    get clientWidth() { return this._inner("width", "border-left-width", "border-right-width"); }
    get clientHeight() { return this._inner("height", "border-top-width", "border-bottom-width"); }
    _inner(dim, a, b) {
      const r = this.getBoundingClientRect();
      const cs = getComputedStyle(this);
      return Math.max(0, Math.round(r[dim] - (parseFloat(cs.getPropertyValue(a)) || 0) - (parseFloat(cs.getPropertyValue(b)) || 0)));
    }
    // Only the page itself scrolls so far.
    get scrollTop() { return this === documentObj.documentElement || this === documentObj.body ? N.scrollY() : 0; }
    set scrollTop(v) { if (this === documentObj.documentElement || this === documentObj.body) N.scrollTo(+v || 0); }
    get scrollLeft() { return 0; }
    scrollIntoView() { const r = N.rect(this._id); if (r) N.scrollTo(N.scrollY() + r[1]); }
  }

  class Document extends Node {
    get nodeName() { return "#document"; }
    get readyState() { return readyState; }
    get documentElement() { return wrap(N.docElement()); }
    get body() { return wrap(N.bodyEl()); }
    get head() { return wrap(N.headEl()); }
    get defaultView() { return g; }
    getElementById(id) { return wrap(N.byId(String(id))); }
    createElement(tag) { return wrap(N.createElement(String(tag))); }
    createTextNode(s) { return wrap(N.createText(String(s))); }
    querySelector(s) { const r = N.select(this._id, String(s)); return r.length ? wrap(r[0]) : null; }
    querySelectorAll(s) { return N.select(this._id, String(s)).map(wrap); }
  }
  let readyState = "loading";
  const documentObj = new Document(N.root());
  cache.set(N.root(), documentObj);

  Object.assign(g, {
    EventTarget, Event, UIEvent, MouseEvent, KeyboardEvent, FocusEvent, CustomEvent,
    Node, Element, HTMLElement: Element, Text, Document, document: documentObj,
    window: g, self: g,
    addEventListener: (...a) => EventTarget.prototype.addEventListener.apply(g, a),
    removeEventListener: (...a) => EventTarget.prototype.removeEventListener.apply(g, a),
    dispatchEvent: (ev) => dispatch(g, ev),
    performance: { now: () => N.now() },
    queueMicrotask: (f) => { Promise.resolve().then(f).catch(report); },
  });

  // ---- computed style, window metrics, scrolling ---------------------------
  g.getComputedStyle = (el) => {
    const get = (n) => {
      n = kebab(n);
      const v = N.computed(el._id, n) ?? null;
      if (v !== null && (n === "width" || n === "height")) {
        const r = N.rect(el._id);
        // The used size is the border box, minus padding/border for content-box.
        if (r) return Math.round((n === "width" ? r[2] : r[3]) * 100) / 100 + "px";
      }
      return v === null ? "" : v;
    };
    return new Proxy({ getPropertyValue: get }, {
      get: (t, k) => (typeof k === "symbol" ? undefined : k in t ? t[k] : get(k)),
    });
  };
  const vp = (i) => N.viewport()[i];
  Object.defineProperties(g, {
    innerWidth: { get: () => vp(0) },
    innerHeight: { get: () => vp(1) },
    scrollY: { get: () => N.scrollY() },
    pageYOffset: { get: () => N.scrollY() },
    scrollX: { value: 0 },
    pageXOffset: { value: 0 },
    devicePixelRatio: { value: 1 },
  });
  const scrollArg = (a, b) => (typeof a === "object" && a !== null ? a.top : b);
  Object.assign(g, {
    scrollTo: (a, b) => { const y = scrollArg(a, b); if (y !== undefined) N.scrollTo(+y || 0); },
    scrollBy: (a, b) => { const y = scrollArg(a, b); if (y !== undefined) N.scrollTo(N.scrollY() + (+y || 0)); },
  });
  g.scroll = g.scrollTo;

  // ---- console -----------------------------------------------------------
  const fmt = (a) => a.map((x) => (typeof x === "string" ? x : (() => { try { return typeof x === "object" && x !== null ? JSON.stringify(x) : String(x); } catch { return String(x); } })())).join(" ");
  g.console = {};
  for (const level of ["log", "info", "debug", "warn", "error"]) g.console[level] = (...a) => N.log(level, fmt(a));

  // ---- timers ------------------------------------------------------------
  let timers = [];
  let nextTimer = 1;
  const schedule = (fn, ms, every, args) => {
    const id = nextTimer++;
    timers.push({ id, due: N.now() + Math.max(0, +ms || 0), every, fn, args });
    return id;
  };
  const clear = (id) => { timers = timers.filter((t) => t.id !== id); };
  Object.assign(g, {
    setTimeout: (fn, ms, ...args) => schedule(fn, ms, -1, args),
    setInterval: (fn, ms, ...args) => schedule(fn, ms, Math.max(1, +ms || 0), args),
    clearTimeout: clear,
    clearInterval: clear,
    requestAnimationFrame: (fn) => schedule(() => fn(N.now()), 16, -1, []),
    cancelAnimationFrame: clear,
  });

  // Run timers due at `now` (ms); returns ms until the next one, or -1.
  Object.defineProperty(g, "__tick", {
    value(now) {
      for (let guard = 0; guard < 1000; guard++) {
        timers.sort((a, b) => a.due - b.due || a.id - b.id);
        const t = timers[0];
        if (!t || t.due > now) break;
        if (t.every >= 0) {
          t.due += t.every;
          if (t.due <= now) t.due = now + t.every; // don't spin catching up
        } else timers.shift();
        try { t.fn(...t.args); } catch (e) { report(e); }
      }
      return timers.length ? Math.max(0, timers[0].due - now) : -1;
    },
  });

  // ---- entry points for the host -----------------------------------------
  const MOUSE = new Set(["click", "dblclick", "mousedown", "mouseup", "mousemove", "mouseover", "mouseout", "contextmenu"]);
  const KEY = new Set(["keydown", "keyup"]);
  Object.defineProperty(g, "__dispatch", {
    value(kind, id, init) {
      const target = wrap(id);
      const o = Object.assign({ bubbles: true, cancelable: true }, init);
      let ev;
      if (MOUSE.has(kind)) ev = new MouseEvent(kind, o);
      else if (KEY.has(kind)) ev = new KeyboardEvent(kind, o);
      else if (kind === "focus" || kind === "blur") ev = new FocusEvent(kind, { ...o, bubbles: false });
      else ev = new Event(kind, o);
      ev.isTrusted = true;
      return dispatch(target, ev);
    },
  });
  Object.defineProperty(g, "__lifecycle", {
    value() {
      readyState = "interactive";
      dispatch(documentObj, new Event("DOMContentLoaded", { bubbles: true }));
      readyState = "complete";
      dispatch(g, new Event("load"));
    },
  });
})(globalThis);
