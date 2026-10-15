// The web-facing DOM, built on a handful of native functions (`__nile`).
// Nodes are thin wrappers around arena ids, cached so `a === b` holds.
"use strict";
(function (g) {
  const N = g.__nile;
  delete g.__nile;

  // QuickJS stacks hold only the frames, so the message goes first.
  // `@debug.setUnhandledExceptionHandler` and `setConsoleOutputHandler` hang their functions here.
  const hooks = { unhandled: null, console: null };
  const report = (e) => {
    if (hooks.unhandled) { const h = hooks.unhandled; hooks.unhandled = null; try { h(e); return; } catch (_) { /* fall through */ } finally { hooks.unhandled = h; } }
    N.log("error", "Uncaught " + (e && e.stack ? (e.message !== undefined ? String(e.name || "Error") + ": " + e.message + "\n" : "") + e.stack : e));
  };
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
  let adopting = -1;
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
    constructor(id) {
      super();
      // A class component (`class Clock extends Element`) is built by `new Clock(props, kids)`:
      // `adopting` hands it the element the engine made for it, or a new `<div>` outside JSX.
      if (typeof id !== "number") {
        this._id = adopting >= 0 ? adopting : N.createElement("div");
        cache.set(this._id, this);
      } else this._id = id;
    }
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
    appendChild(c) { N.append(this._id, idOf(c)); mounted(); return c; }
    insertBefore(c, ref) { N.insertBefore(this._id, idOf(c), ref ? idOf(ref) : -1); mounted(); return c; }
    removeChild(c) {
      if (c.parentNode !== this) throw new Error("NotFoundError: the node is not a child of this node");
      unmounting(c);
      N.remove(c._id);
      return c;
    }
    remove() { unmounting(this); N.remove(this._id); }
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
    // Adds or removes an attribute (`force` picks which); true if it is present afterwards.
    toggleAttribute(n, force) {
      const on = force === undefined ? !this.hasAttribute(n) : !!force;
      if (on) { if (!this.hasAttribute(n)) this.setAttribute(n, ""); } else this.removeAttribute(n);
      return on;
    }
    getAttributeNames() { return N.attrNames(this._id); }
    get children() { return this.childNodes.filter((n) => n.nodeType === 1); }
    get childElementCount() { return this.children.length; }
    get firstElementChild() { return this.children[0] || null; }
    get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }
    get nextElementSibling() { for (let n = this.nextSibling; n; n = n.nextSibling) if (n.nodeType === 1) return n; return null; }
    get previousElementSibling() { for (let n = this.previousSibling; n; n = n.previousSibling) if (n.nodeType === 1) return n; return null; }
    get innerHTML() { return N.innerHTML(this._id); }
    set innerHTML(v) { N.setInnerHTML(this._id, String(v)); }
    // `append`, `prepend` and `content` take vnodes (from JSX), strings/numbers
    // (text), DOM nodes, or arrays of those; null/false are skipped.
    append(...items) { for (const n of materialize(expand(items, this))) this.appendChild(n); }
    prepend(...items) {
      const first = this.firstChild;
      for (const n of materialize(expand(items, this))) this.insertBefore(n, first);
    }
    content(...items) {
      const nodes = materialize(expand(items, this));
      if (liveComps) for (const c of this.childNodes) unmounting(c);
      N.setText(this._id, "");
      for (const n of nodes) this.appendChild(n);
      return this;
    }
    // Sciter: reconcile this element with a vnode, reusing matching DOM.
    // Simplified: children match by key, id, then tag, in order.
    patch(vnode, onlyChildren = false) {
      const items = expand(vnode, this.parentElement);
      if (onlyChildren) return patchChildren(this, items), this;
      const root = items.find((i) => i && i.tag);
      if (root) {
        applyAttrs(this, root.attrs, currentAttrs(this));
        patchChildren(this, root.kids);
      }
      return this;
    }
    // Class components: overridden by subclasses (see "class components" below).
    this() {}
    render() { return null; }
    post(fn, avoidDuplicates) {
      if (avoidDuplicates && this._posted && this._posted.has(fn)) return;
      (this._posted = this._posted || new Set()).add(fn);
      queueMicrotask(() => { this._posted.delete(fn); fn.call(this, this); });
    }
    // Merge `obj` into the element and re-render later when something changed (always when
    // called without one); batches several calls into one render.
    componentUpdate(obj) {
      let changed = obj === undefined || obj === null;
      if (obj && typeof obj === "object") {
        for (const [k, v] of Object.entries(obj)) if (this[k] !== v) { this[k] = v; changed = true; }
      }
      if (!changed || this._upd) return;
      this._upd = true;
      queueMicrotask(() => {
        this._upd = false;
        if (!this._comp) return;
        draw(this, this.render());
        if (typeof this.componentDidUpdate === "function") this.componentDidUpdate();
      });
    }
    static create(v) {
      if (typeof v === "string") return documentObj.createElement(v);
      const el = materialize(expand(v, null)).find((n) => n.nodeType === 1);
      mounted();
      return el || null;
    }
    insertAfter(node, ref) { this.insertBefore(node, ref ? ref.nextSibling : this.firstChild); return node; }
    get elementIndex() { const p = this.parentElement; return p ? p.children.indexOf(this) : -1; }
    get nodeIndex() { const p = this.parentNode; return p ? p.childNodes.indexOf(this) : -1; }
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
    // Elements with non-visible `overflow` scroll; the root and <body> scroll the page.
    get scrollTop() { return this._isRoot() ? N.scrollY() : (N.elScroll(this._id) || [0, 0])[1]; }
    set scrollTop(v) { if (this._isRoot()) N.scrollTo(+v || 0); else { const s = N.elScroll(this._id); if (s) N.elScrollTo(this._id, s[0], +v || 0); } }
    // `page-view: paged` elements: how many sheets of paper they show and which one has the caret
    // (else the one in the middle of the window); 0 for other elements.
    get pageCount() { return (N.pageInfo(this._id) || [0, 0])[0]; }
    get pageNumber() { return (N.pageInfo(this._id) || [0, 0])[1]; }
    get scrollLeft() { return this._isRoot() ? 0 : (N.elScroll(this._id) || [0])[0]; }
    set scrollLeft(v) { if (!this._isRoot()) { const s = N.elScroll(this._id); if (s) N.elScrollTo(this._id, +v || 0, s[1]); } }
    get scrollWidth() { const s = N.elScroll(this._id); return s ? Math.round(s[2]) : this.clientWidth; }
    get scrollHeight() { const s = N.elScroll(this._id); return s ? Math.round(s[3]) : this.clientHeight; }
    _isRoot() { return this === documentObj.documentElement || this === documentObj.body; }
    scrollTo(a, b) {
      const s = N.elScroll(this._id);
      if (!s) return;
      if (typeof a === "object" && a !== null) N.elScrollTo(this._id, a.left ?? s[0], a.top ?? s[1]);
      else N.elScrollTo(this._id, +a || 0, +b || 0);
    }
    scrollBy(a, b) {
      const s = N.elScroll(this._id);
      if (!s) return;
      const [dx, dy] = typeof a === "object" && a !== null ? [a.left || 0, a.top || 0] : [+a || 0, +b || 0];
      N.elScrollTo(this._id, s[0] + dx, s[1] + dy);
    }
    // Scrolls the scrollable ancestors just enough to show the element, then the page to its top.
    scrollIntoView() {
      for (let p = this.parentElement; p; p = p.parentElement) {
        const s = N.elScroll(p._id), pr = N.rect(p._id), r = N.rect(this._id);
        if (!s || !pr || !r) continue;
        const top = r[1] - pr[1];
        if (top < 0) N.elScrollTo(p._id, s[0], s[1] + top);
        else if (top + r[3] > pr[3]) N.elScrollTo(p._id, s[0], s[1] + top + r[3] - pr[3]);
      }
      const r = N.rect(this._id);
      if (r) N.scrollTo(N.scrollY() + r[1]);
    }
  }

  // ---- vnodes -------------------------------------------------------------
  // JSX produces vnodes: arrays [tag, attrs, children] (Sciter's shape).
  // `expand` resolves components and fragments into plain {tag, attrs, kids}
  // objects, strings and DOM nodes; `materialize` turns those into DOM nodes.
  const isVNode = (x) => Array.isArray(x) && x.$v === true;

  function expand(x, parent, out = []) {
    if (x == null || x === false || x === true) return out;
    if (typeof x === "string" || typeof x === "number") out.push(String(x));
    else if (x instanceof Node) out.push(x);
    else if (isVNode(x)) {
      const [tag, attrs, kids] = x;
      if (tag === JSX.Fragment) expand(kids, parent, out);
      else if (typeof tag === "function" && tag.prototype instanceof Element) {
        out.push({ tag: "div", attrs: {}, kids: [], comp: tag, props: attrs || {}, children: flat(kids) });
      } else if (typeof tag === "function") expand(tag(attrs || {}, flat(kids), parent), parent, out);
      else out.push({ tag: String(tag), attrs: attrs || {}, kids: expand(kids, null) });
    } else if (Array.isArray(x)) for (const y of x) expand(y, parent, out);
    else out.push(String(x));
    return out;
  }

  function flat(x, out = []) {
    if (Array.isArray(x) && !isVNode(x)) for (const y of x) flat(y, out);
    else if (x != null && x !== false) out.push(x);
    return out;
  }

  function materialize(items) {
    return items.map((it) => {
      if (typeof it === "string") return documentObj.createTextNode(it);
      if (it instanceof Node) return it;
      if (it.comp) return instantiate(it);
      const el = documentObj.createElement(it.tag);
      applyAttrs(el, it.attrs, null);
      for (const k of materialize(it.kids)) el.appendChild(k);
      if (typeof it.attrs.ref === "function") it.attrs.ref(el);
      return el;
    });
  }

  // ---- class components ---------------------------------------------------
  // `class Clock extends Element { render() { ... } }` used as `<Clock/>`: the element is a
  // `<div>` whose prototype is the class. Mounting runs the constructor, `this(props, kids)`,
  // `render(props, kids)`, then `componentDidMount` once the element is in the document;
  // a parent's render runs `this(...)` + `render(...)` again; `componentUpdate` re-renders
  // later (`componentDidUpdate` after); removal calls `componentWillUnmount`.
  let liveComps = 0;
  const mountQueue = [];
  const plainProps = (props) => {
    const a = {};
    for (const [k, v] of Object.entries(props)) if (typeof v !== "object" && (typeof v !== "function" || /^on[A-Za-z]/.test(k))) a[k] = v;
    return a;
  };
  function instantiate(it) {
    const id = N.createElement("div");
    adopting = id;
    let el;
    try { el = new it.comp(it.props, it.children); } finally { adopting = -1; }
    el._comp = true;
    liveComps++;
    el._pa = plainProps(it.props);
    applyAttrs(el, el._pa, null);
    el.this(it.props, it.children);
    draw(el, el.render(it.props, it.children));
    mountQueue.push(el);
    if (typeof it.props.ref === "function") it.props.ref(el);
    return el;
  }
  function refresh(el, it) {
    el._pa = plainProps(it.props);
    el.this(it.props, it.children);
    draw(el, el.render(it.props, it.children));
  }
  // Show what `render` returned: the root vnode's attributes (over the props') and children
  // go onto the component's own element, as `patch` does; anything else becomes its content.
  function draw(el, vnode) {
    const items = expand(vnode, null);
    const root = items.find((i) => i && i.tag);
    if (root && items.length === 1) {
      applyAttrs(el, Object.assign({}, el._pa, root.attrs), currentAttrs(el));
      patchChildren(el, root.kids);
    } else {
      applyAttrs(el, el._pa, currentAttrs(el));
      patchChildren(el, items);
    }
  }
  function mounted() {
    if (!mountQueue.length) return;
    for (let i = 0; i < mountQueue.length;) {
      const el = mountQueue[i];
      if (!el.isConnected) { i++; continue; }
      mountQueue.splice(i, 1);
      if (typeof el.componentDidMount === "function") { try { el.componentDidMount(); } catch (e) { report(e); } }
    }
  }
  function unmounting(node) {
    if (!liveComps || node.nodeType !== 1) return;
    for (const d of [node, ...node.querySelectorAll("*")]) {
      if (!d._comp) continue;
      d._comp = false;
      liveComps--;
      if (typeof d.componentWillUnmount === "function") { try { d.componentWillUnmount(); } catch (e) { report(e); } }
    }
  }

  // What `patch` compares against: the element's real attributes plus JSX-set handlers.
  const currentAttrs = (el) => Object.assign(Object.fromEntries(el.getAttributeNames().map((n) => [n, 1])), el._vattrs);

  // Apply vnode attributes; with `old` (the previous attrs), drop what's gone.
  function applyAttrs(el, attrs, old) {
    el._h = el._h || {};
    const setOn = (type, fn) => {
      if (el._h[type]) el.removeEventListener(type, el._h[type]);
      el._h[type] = fn;
      if (fn) el.addEventListener(type, fn);
    };
    for (const k of Object.keys(old || {})) {
      if (k in attrs) continue;
      if (/^on[A-Za-z]/.test(k)) setOn(k.slice(2).toLowerCase(), null);
      else if (k !== "key" && k !== "ref") el.removeAttribute(k === "className" ? "class" : k);
    }
    for (const [k, v] of Object.entries(attrs)) {
      if (k === "key") { el._key = v; continue; }
      if (k === "ref") continue;
      if (/^on[A-Za-z]/.test(k) && (typeof v === "function" || v == null)) { setOn(k.slice(2).toLowerCase(), v || null); continue; }
      const name = k === "className" ? "class" : k;
      if (v == null || v === false) el.removeAttribute(name);
      else if (name === "style" && typeof v === "object") el.setAttribute("style", Object.entries(v).map(([p, val]) => kebab(p) + ": " + val).join("; "));
      else if (name === "class" && typeof v !== "string") {
        el.setAttribute("class", Array.isArray(v) ? v.filter(Boolean).join(" ") : Object.keys(v).filter((c) => v[c]).join(" "));
      } else el.setAttribute(name, v === true ? "" : String(v));
    }
    el._vattrs = attrs;
  }

  function patchChildren(el, items) {
    const dom = el.childNodes;
    const used = new Set();
    const result = [];
    for (const it of items) {
      let node;
      if (typeof it === "string") {
        node = dom.find((n) => !used.has(n) && n.nodeType === 3);
        if (node) { if (node.data !== it) node.data = it; } else node = documentObj.createTextNode(it);
      } else if (it instanceof Node) node = it;
      else if (it.comp) {
        const key = it.props.key;
        node = dom.find((n) => !used.has(n) && n instanceof it.comp && (key === undefined ? n._key === undefined : n._key === key));
        if (node) refresh(node, it); else node = instantiate(it);
      } else {
        const key = it.attrs.key;
        node = dom.find((n) => !used.has(n) && n.nodeType === 1 && n.localName === it.tag.toLowerCase() &&
          (key === undefined ? n._key === undefined : n._key === key) && (!it.attrs.id || n.id === String(it.attrs.id)));
        if (node) {
          applyAttrs(node, it.attrs, currentAttrs(node));
          patchChildren(node, it.kids);
        } else node = materialize([it])[0];
      }
      used.add(node);
      result.push(node);
    }
    for (const n of dom) if (!used.has(n)) el.removeChild(n);
    result.forEach((n, i) => {
      const cur = el.childNodes[i];
      if (cur !== n) el.insertBefore(n, cur || null);
    });
  }

  class Document extends Node {
    get nodeName() { return "#document"; }
    get readyState() { return readyState; }
    get documentElement() { return wrap(N.docElement()); }
    get body() { return wrap(N.bodyEl()); }
    get head() { return wrap(N.headEl()); }
    get defaultView() { return g; }
    // The window title: the text of <title>, which the host shows in the title bar.
    get title() {
      const t = this.querySelector("title");
      return t ? t.textContent.replace(/\s+/g, " ").trim() : "";
    }
    set title(v) {
      let t = this.querySelector("title");
      if (!t) { t = this.createElement("title"); (this.head || this.documentElement).appendChild(t); }
      t.textContent = String(v);
    }
    get fullscreenElement() { return N.windowRect()[4] ? this.documentElement : null; }
    exitFullscreen() { N.windowCommand("fullscreen", 0, 0); return Promise.resolve(); }
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

  // ---- form controls -------------------------------------------------------
  // `value` is stored in the `value` attribute (a simplification: there is no
  // separate "default value"). Elements without a native behavior have no
  // `value` (undefined), as in Sciter, but can be given one as an ordinary property.
  const VALUE_TAGS = new Set(["input", "textarea", "button"]);
  // `<form>` (or `behavior="form"`): `value` is an object of the named controls' values, as in
  // Sciter: text as strings, checkboxes as booleans, a radio group as the chosen value,
  // numbers/ranges as numbers; assigning an object sets the controls it names.
  const isForm = (el) => el.localName === "form" || el.getAttribute("behavior") === "form";
  const formControls = (el) => el.querySelectorAll("input, select, textarea").filter((c) => c.getAttribute("name"));
  const typeOf = (c) => (c.localName === "input" ? (c.getAttribute("type") || "text").toLowerCase() : c.localName);
  function formValue(form) {
    const out = {};
    for (const c of formControls(form)) {
      const name = c.getAttribute("name"), t = typeOf(c);
      if (t === "checkbox") out[name] = !!c.checked;
      else if (t === "radio") { if (c.checked) out[name] = c.value; else if (!(name in out)) out[name] = undefined; }
      else if (t === "number" || t === "range") out[name] = c.value === "" ? null : Number(c.value);
      else out[name] = c.value;
    }
    return out;
  }
  function setFormValue(form, obj) {
    for (const c of formControls(form)) {
      const name = c.getAttribute("name");
      if (!obj || !(name in obj)) continue;
      const v = obj[name], t = typeOf(c);
      if (t === "checkbox") c.checked = !!v;
      else if (t === "radio") c.checked = String(c.value) === String(v);
      else c.value = v == null ? "" : String(v);
    }
  }
  const optValue = (o) => o.getAttribute("value") ?? o.text;
  Object.defineProperty(Element.prototype, "value", {
    configurable: true,
    get() {
      const t = this.localName;
      if (t === "select") { const o = this.options[this.selectedIndex]; return o ? optValue(o) : ""; }
      if (t === "option") return optValue(this);
      if (!VALUE_TAGS.has(t)) return isForm(this) ? formValue(this) : undefined;
      const v = this.getAttribute("value");
      if (v === null && t === "textarea") return this.textContent;
      if (v === null && t === "input" && this.type === "color") return "#000000";
      if (v === null && t === "input" && this.type === "range") {
        const lo = +(this.getAttribute("min") ?? 0), hi = +(this.getAttribute("max") ?? 100);
        return String(Math.min(Math.max((lo + hi) / 2, lo), Math.max(lo, hi)));
      }
      return v !== null ? v : t === "input" && /^(checkbox|radio)$/.test(this.type) ? "on" : "";
    },
    set(v) {
      const t = this.localName;
      if (t === "select") this.selectedIndex = this.options.findIndex((o) => optValue(o) === String(v));
      else if (t === "option" || VALUE_TAGS.has(t)) { this.setAttribute("value", v); N.valueSynced(this._id); }
      else if (isForm(this)) setFormValue(this, v);
      else Object.defineProperty(this, "value", { value: v, writable: true, configurable: true, enumerable: true });
    },
  });

  // <select> / <option>. The chosen option carries the `selected` state flag; a
  // select with none flagged shows its first enabled option, as browsers do.
  Object.defineProperty(Element.prototype, "options", {
    configurable: true,
    get() { return this.localName === "select" ? this.querySelectorAll("option") : undefined; },
  });
  Object.defineProperty(Element.prototype, "selectedIndex", {
    configurable: true,
    get() {
      if (this.localName !== "select") return undefined;
      const opts = this.options;
      let i = opts.findIndex((o) => N.getState(o._id, "selected"));
      // A multiple list box may have nothing chosen; a drop-down always shows something.
      if (i < 0 && this.hasAttribute("multiple")) return -1;
      if (i < 0) i = opts.findIndex((o) => !N.getState(o._id, "disabled"));
      return i < 0 && opts.length ? 0 : i;
    },
    set(idx) {
      if (this.localName !== "select") return;
      this.options.forEach((o, i) => {
        N.setState(o._id, "selected", i === idx);
        if (i !== idx) o.removeAttribute("selected");
      });
    },
  });
  Object.defineProperty(Element.prototype, "selected", {
    configurable: true,
    get() {
      if (this.localName !== "option") return undefined;
      const s = this.closest("select");
      if (s && s.hasAttribute("multiple")) return N.getState(this._id, "selected");
      return s ? s.options[s.selectedIndex] === this : N.getState(this._id, "selected");
    },
    set(v) {
      if (this.localName !== "option") return;
      const s = this.closest("select");
      if (s && v && !s.hasAttribute("multiple")) s.selectedIndex = s.options.indexOf(this);
      else { N.setState(this._id, "selected", !!v); if (!v) this.removeAttribute("selected"); }
    },
  });
  Object.defineProperty(Element.prototype, "selectedOptions", {
    configurable: true,
    get() { return this.localName === "select" ? this.options.filter((o) => N.getState(o._id, "selected")) : undefined; },
  });
  Object.defineProperty(Element.prototype, "text", {
    configurable: true,
    get() { return this.localName === "option" ? this.textContent.replace(/\s+/g, " ").trim() : this.textContent; },
    set(v) { this.textContent = String(v); },
  });
  Object.defineProperty(Element.prototype, "label", {
    configurable: true,
    get() { return this.localName === "option" ? this.getAttribute("label") || this.text : undefined; },
  });
  Object.defineProperty(Element.prototype, "length", {
    configurable: true,
    get() { return this.localName === "select" ? this.options.length : undefined; },
  });
  const reflect = (prop, attr, dflt = "") =>
    Object.defineProperty(Element.prototype, prop, {
      configurable: true,
      get() { return this.getAttribute(attr) ?? dflt; },
      set(v) { this.setAttribute(attr, v); },
    });
  // Table cells: `colSpan` / `rowSpan` (at least 1), and `cellIndex`.
  for (const [prop, attr] of [["colSpan", "colspan"], ["rowSpan", "rowspan"]]) {
    Object.defineProperty(Element.prototype, prop, {
      configurable: true,
      get() { return this.localName === "td" || this.localName === "th" ? Math.max(1, parseInt(this.getAttribute(attr) || "1", 10) || 1) : undefined; },
      set(v) { this.setAttribute(attr, String(Math.max(1, v | 0))); },
    });
  }
  Object.defineProperty(Element.prototype, "cellIndex", {
    configurable: true,
    get() { return this.localName === "td" || this.localName === "th" ? Array.from(this.parentNode.children).indexOf(this) : undefined; },
  });
  reflect("placeholder", "placeholder");
  reflect("name", "name");
  Object.defineProperty(Element.prototype, "type", {
    configurable: true,
    get() { const t = this.getAttribute("type"); return t !== null ? t.toLowerCase() : this.localName === "input" ? "text" : this.localName === "button" ? "submit" : ""; },
    set(v) { this.setAttribute("type", v); },
  });
  // `checked` / `disabled` are runtime state flags; clearing also drops the HTML attribute.
  for (const flag of ["checked", "disabled"]) {
    Object.defineProperty(Element.prototype, flag, {
      configurable: true,
      get() { return N.getState(this._id, flag); },
      set(v) {
        N.setState(this._id, flag, !!v);
        if (!v) this.removeAttribute(flag);
      },
    });
  }
  // Selection in text controls, in characters of `value` (UTF-16 units in browsers; same for the BMP).
  const TEXT_TAGS = new Set(["input", "textarea"]);
  Object.defineProperty(Element.prototype, "selectionStart", {
    configurable: true,
    get() { return TEXT_TAGS.has(this.localName) ? N.selection(this._id)[0] : null; },
    set(v) { if (TEXT_TAGS.has(this.localName)) N.setSelection(this._id, +v || 0, N.selection(this._id)[1]); },
  });
  Object.defineProperty(Element.prototype, "selectionEnd", {
    configurable: true,
    get() { return TEXT_TAGS.has(this.localName) ? N.selection(this._id)[1] : null; },
    set(v) { if (TEXT_TAGS.has(this.localName)) N.setSelection(this._id, N.selection(this._id)[0], +v || 0); },
  });
  Element.prototype.setSelectionRange = function (start, end) {
    if (TEXT_TAGS.has(this.localName)) N.setSelection(this._id, +start || 0, +end || 0);
  };
  Element.prototype.select = function () {
    if (TEXT_TAGS.has(this.localName)) N.setSelection(this._id, 0, this.value.length);
  };
  Element.prototype.focus = function () { N.focus(this._id); };
  Element.prototype.blur = function () { if (N.activeElement() === this._id) N.focus(-1); };
  Object.defineProperty(Document.prototype, "activeElement", { get() { const id = N.activeElement(); return id < 0 ? this.body : wrap(id); } });

  // ---- JSX ---------------------------------------------------------------
  // `<div class="a">hi</div>` compiles to JSX("div", {class: "a"}, ["hi"]).
  // Like Sciter, the default returns a vnode array that append/content/patch
  // render; function components are called as Component(props, kids, parent).
  // Replace `globalThis.JSX` to change what JSX means (e.g. Mithril's `m`).
  function JSX(tag, attrs, kids) {
    const v = [tag, attrs, kids];
    Object.defineProperty(v, "$v", { value: true });
    return v;
  }
  JSX.Fragment = function Fragment() {};
  g.JSX = JSX;

  // ---- Sciter-style helpers ----------------------------------------------
  g.$ = (s) => documentObj.querySelector(s);
  g.$$ = (s) => documentObj.querySelectorAll(s);
  for (const C of [Element, Document]) {
    C.prototype.$ = C.prototype.querySelector;
    C.prototype.$$ = C.prototype.querySelectorAll;
  }

  // on("click mousedown.ns ^keydown", [selector], handler)
  //   handler(evt, matched): `this` is the element it is attached to; `matched`
  //   is the descendant matching `selector` (or the element itself).
  //   A leading ^ subscribes in the capturing phase; `.ns` adds a namespace.
  const subs = new WeakMap(); // element -> [{ type, ns, capture, handler, wrapper }]
  const parseSpec = (spec) => {
    const capture = spec.startsWith("^");
    const [type, ns = ""] = spec.replace(/^\^/, "").split(".");
    return { type, ns, capture };
  };
  function subscribe(el, types, selector, handler, once) {
    if (typeof selector === "function") [selector, handler] = [null, selector];
    for (const spec of String(types).split(/\s+/).filter(Boolean)) {
      const { type, ns, capture } = parseSpec(spec);
      const wrapper = (ev) => {
        let m = el;
        if (selector) {
          // The nearest element from the target up to `el` that the selector picks, read from `el`
          // (so `>strip>label` and `:root>x` work).
          m = null;
          for (let n = ev.target instanceof Element ? ev.target : null; n && n !== el; n = n.parentElement) {
            if (el.nodeType === 1 ? N.matchesIn(n._id, el._id, selector) : n.matches(selector)) { m = n; break; }
          }
          if (!m) return;
        }
        if (once) off(el, null, handler);
        return handler.call(el, ev, m);
      };
      let list = subs.get(el);
      if (!list) subs.set(el, (list = []));
      list.push({ type, ns, capture, handler, wrapper });
      el.addEventListener(type, wrapper, capture);
    }
    return el;
  }
  // off("click"), off(".ns"), off(handler), off("click", handler)
  function off(el, what, handler) {
    if (typeof what === "function") [what, handler] = [null, what];
    const want = what ? parseSpec(String(what)) : null;
    const list = subs.get(el) || [];
    for (const rec of list.slice()) {
      if (handler && rec.handler !== handler) continue;
      if (want && ((want.type && rec.type !== want.type) || (want.ns && rec.ns !== want.ns))) continue;
      el.removeEventListener(rec.type, rec.wrapper, rec.capture);
      list.splice(list.indexOf(rec), 1);
    }
    return el;
  }
  EventTarget.prototype.on = function (types, selector, handler) { return subscribe(this, types, selector, handler, false); };
  EventTarget.prototype.once = function (types, selector, handler) { return subscribe(this, types, selector, handler, true); };
  EventTarget.prototype.off = function (what, handler) { return off(this, what, handler); };

  // Element.state: runtime flags. hover/active/focus come from interaction;
  // the others (checked, disabled, readonly, expanded, ...) are stored on the
  // element and matched by the same-named pseudo-classes.
  const FLAGS = ["checked", "disabled", "readonly", "expanded", "collapsed", "current", "selected", "invalid", "busy", "valid", "visited", "focusable", "tabfocus"];
  Object.defineProperty(Element.prototype, "state", {
    get() {
      return new Proxy({}, {
        get: (_, k) => (k === "hover" || k === "active" || k === "focus" ? this.matches(":" + k) : FLAGS.includes(k) ? N.getState(this._id, k) : undefined),
        set: (_, k, v) => { if (FLAGS.includes(k)) N.setState(this._id, k, !!v); return true; },
        has: (_, k) => FLAGS.includes(k) || k === "hover" || k === "active" || k === "focus",
      });
    },
  });

  // Element.attributes: an array-like of { name, value } (a simplified NamedNodeMap).
  Object.defineProperty(Element.prototype, "attributes", {
    get() {
      const attrs = this.getAttributeNames().map((name) => {
        const a = { name };
        Object.defineProperty(a, "value", { enumerable: true, get: () => this.getAttribute(name), set: (v) => this.setAttribute(name, v) });
        return a;
      });
      attrs.getNamedItem = (n) => attrs.find((a) => a.name === String(n).toLowerCase()) || null;
      // `attributes[i]` is an `{ name, value }`; `attributes.name` is the attribute's value as a string, as in
      // Sciter (`label.attributes["for"]`), and assigning to it sets the attribute.
      return new Proxy(attrs, {
        get: (t, k) => (k in t || typeof k === "symbol" ? t[k] : this.getAttribute(String(k).toLowerCase()) ?? undefined),
        set: (t, k, v) => { if (typeof k === "string" && !(k in t)) this.setAttribute(k, v); return true; },
      });
    },
  });

  // ---- computed style, window metrics, scrolling ---------------------------
  // `window.getSelection()`: the text selected on the page (not inside inputs).
  g.getSelection = () => ({
    toString: () => N.pageSelection(),
    get isCollapsed() { return N.pageSelection() === ""; },
    get rangeCount() { return N.pageSelection() === "" ? 0 : 1; },
    removeAllRanges: () => N.clearPageSelection(),
    empty: () => N.clearPageSelection(),
  });
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
    devicePixelRatio: { get: () => N.scale() },
  });
  // The host window (desktop, so these really move and resize it).
  const wr = (i) => N.windowRect()[i];
  Object.defineProperties(g, {
    screenX: { get: () => wr(0) },
    screenY: { get: () => wr(1) },
    screenLeft: { get: () => wr(0) },
    screenTop: { get: () => wr(1) },
    outerWidth: { get: () => wr(2) },
    outerHeight: { get: () => wr(3) },
  });
  Object.assign(g, {
    close: () => N.windowCommand("close", 0, 0),
    moveTo: (x, y) => N.windowCommand("moveTo", +x || 0, +y || 0),
    moveBy: (dx, dy) => N.windowCommand("moveTo", wr(0) + (+dx || 0), wr(1) + (+dy || 0)),
    // Sizes are the page area, so resizing by N changes the viewport by N.
    resizeTo: (w, h) => N.windowCommand("resizeTo", +w || 0, +h || 0),
    resizeBy: (dw, dh) => N.windowCommand("resizeTo", N.viewport()[0] + (+dw || 0), N.viewport()[1] + (+dh || 0)),
  });
  Element.prototype.requestFullscreen = function () { N.windowCommand("fullscreen", 1, 0); return Promise.resolve(); };
  const scrollArg = (a, b) => (typeof a === "object" && a !== null ? a.top : b);
  Object.assign(g, {
    scrollTo: (a, b) => { const y = scrollArg(a, b); if (y !== undefined) N.scrollTo(+y || 0); },
    scrollBy: (a, b) => { const y = scrollArg(a, b); if (y !== undefined) N.scrollTo(N.scrollY() + (+y || 0)); },
  });
  g.scroll = g.scrollTo;

  // ---- console -----------------------------------------------------------
  const fmt = (a) => a.map((x) => (typeof x === "string" ? x : (() => { try { return typeof x === "object" && x !== null ? JSON.stringify(x) : String(x); } catch { return String(x); } })())).join(" ");
  g.console = {};
  for (const level of ["log", "info", "debug", "warn", "error"]) g.console[level] = (...a) => {
    const msg = fmt(a);
    if (hooks.console) {
      const h = hooks.console; hooks.console = null;
      try { if (h("script", level === "error" ? 2 : level === "warn" ? 1 : 0, msg) === true) return; } catch (_) { /* log normally */ } finally { hooks.console = h; }
    }
    N.log(level, msg);
  };

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

  /*@EDIT@*/

  /*@WEB@*/

  // ---- the embedder ------------------------------------------------------
  // `host.name(...args)` calls a function the embedding application registered; arguments and
  // result travel as JSON. `host.has(name)` tells whether one exists.
  const hostCall = (name, args) => {
    const r = N.hostCall(String(name), JSON.stringify(args));
    return r === "" ? undefined : JSON.parse(r);
  };
  g.host = new Proxy({}, {
    get: (_, k) => {
      if (typeof k === "symbol") return undefined;
      if (k === "call") return (name, ...args) => hostCall(name, args);
      if (k === "has") return (name) => N.hostHas(String(name));
      return (...args) => hostCall(k, args);
    },
  });
  Object.defineProperty(g, "__show", {
    value: (v) => (typeof v === "string" ? v : v === undefined ? "" : (() => { try { return JSON.stringify(v) ?? ""; } catch { return String(v); } })()),
  });
  Object.defineProperty(g, "__invoke", {
    value(name, argsJson) {
      const parts = String(name).split(".");
      const fn = parts.reduce((o, k) => (o == null ? undefined : o[k]), g);
      if (typeof fn !== "function") throw new Error(`${name} is not a function`);
      const owner = parts.length > 1 ? parts.slice(0, -1).reduce((o, k) => o[k], g) : g;
      const r = fn.apply(owner, JSON.parse(argsJson || "[]"));
      return r === undefined ? "" : JSON.stringify(r) ?? "";
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
      else if (kind === "scroll") ev = new Event(kind, { ...o, bubbles: false, cancelable: false });
      else ev = new Event(kind, o);
      if (kind === "transitionend" || kind === "animationstart" || kind === "animationend") {
        for (const k of ["propertyName", "animationName", "elapsedTime", "pseudoElement"]) if (k in init) ev[k] = init[k];
      }
      ev.isTrusted = true;
      return dispatch(target, ev);
    },
  });
  Object.defineProperty(g, "__windowEvent", { value(kind) { dispatch(g, new Event(kind)); } });
  Object.defineProperty(g, "__lifecycle", {
    value() {
      readyState = "interactive";
      dispatch(documentObj, new Event("DOMContentLoaded", { bubbles: true }));
      readyState = "complete";
      dispatch(g, new Event("load"));
    },
  });
})(globalThis);
