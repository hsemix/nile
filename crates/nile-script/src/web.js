  // ---- web platform odds and ends ---------------------------------------------
  // Spliced into the prelude at `/*@WEB@*/`, so it shares `g`, `N`, `wrap` and `report`.
  // A block of its own keeps its helper names out of the prelude's scope.
  {

  // base64
  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  g.btoa = (s) => {
    s = String(s);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("InvalidCharacterError: the string contains characters outside of the Latin1 range");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  };
  g.atob = (s) => {
    s = String(s).replace(/[\t\n\f\r ]/g, "").replace(/=+$/, "");
    if (/[^A-Za-z0-9+/]/.test(s) || s.length % 4 === 1) throw new Error("InvalidCharacterError: the string is not correctly encoded");
    let out = "", bits = 0, acc = 0;
    for (const ch of s) {
      acc = (acc << 6) | B64.indexOf(ch);
      bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((acc >> bits) & 255); }
    }
    return out;
  };

  // UTF-8
  g.TextEncoder = class TextEncoder {
    get encoding() { return "utf-8"; }
    encode(s = "") {
      const out = [];
      for (const ch of String(s)) {
        let c = ch.codePointAt(0);
        if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
        if (c < 0x80) out.push(c);
        else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return Uint8Array.from(out);
    }
  };
  g.TextDecoder = class TextDecoder {
    constructor(label = "utf-8") { this.encoding = String(label).toLowerCase().replace("utf8", "utf-8"); }
    decode(bytes) {
      const b = bytes instanceof ArrayBuffer ? new Uint8Array(bytes) : bytes ? new Uint8Array(bytes.buffer || bytes, bytes.byteOffset || 0, bytes.byteLength) : [];
      let out = "";
      for (let i = 0; i < b.length;) {
        const c = b[i];
        let n = c < 0x80 ? 0 : c >= 0xf0 ? 3 : c >= 0xe0 ? 2 : c >= 0xc0 ? 1 : -1;
        if (n < 0 || i + n >= b.length) { out += "\ufffd"; i++; continue; }
        let cp = n === 0 ? c : c & (0x3f >> n);
        for (let k = 1; k <= n; k++) cp = (cp << 6) | (b[i + k] & 63);
        out += String.fromCodePoint(cp);
        i += n + 1;
      }
      return out;
    }
  };

  // URL
  const enc = (s) => encodeURIComponent(s).replace(/%20/g, "+").replace(/[!'()~]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());
  const dec = (s) => { try { return decodeURIComponent(s.replace(/\+/g, " ")); } catch { return s; } };
  g.URLSearchParams = class URLSearchParams {
    constructor(init = "") {
      this._list = [];
      this._url = null;
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const i = part.indexOf("=");
          this._list.push(i < 0 ? [dec(part), ""] : [dec(part.slice(0, i)), dec(part.slice(i + 1))]);
        }
      } else if (init && typeof init[Symbol.iterator] === "function") for (const [k, v] of init) this._list.push([String(k), String(v)]);
      else if (init) for (const k of Object.keys(init)) this._list.push([k, String(init[k])]);
    }
    _sync() { if (this._url) this._url._search = this._list.length ? "?" + this.toString() : ""; }
    append(k, v) { this._list.push([String(k), String(v)]); this._sync(); }
    delete(k) { this._list = this._list.filter((e) => e[0] !== k); this._sync(); }
    get(k) { const e = this._list.find((e) => e[0] === k); return e ? e[1] : null; }
    getAll(k) { return this._list.filter((e) => e[0] === k).map((e) => e[1]); }
    has(k) { return this._list.some((e) => e[0] === k); }
    set(k, v) {
      k = String(k); v = String(v);
      const i = this._list.findIndex((e) => e[0] === k);
      if (i < 0) this._list.push([k, v]);
      else { this._list[i][1] = v; this._list = this._list.filter((e, j) => j <= i || e[0] !== k); }
      this._sync();
    }
    sort() { this._list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)); this._sync(); }
    forEach(fn, self) { for (const [k, v] of this._list) fn.call(self, v, k, this); }
    keys() { return this._list.map((e) => e[0])[Symbol.iterator](); }
    values() { return this._list.map((e) => e[1])[Symbol.iterator](); }
    entries() { return this._list.map((e) => [e[0], e[1]])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    get size() { return this._list.length; }
    toString() { return this._list.map(([k, v]) => enc(k) + "=" + enc(v)).join("&"); }
  };
  const DEFAULT_PORTS = { "http:": "80", "https:": "443", "ftp:": "21", "ws:": "80", "wss:": "443" };
  g.URL = class URL {
    constructor(url, base) {
      url = String(url).trim();
      let m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(\/\/([^/?#]*))?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(url);
      if (!m) {
        if (base === undefined) throw new TypeError("Invalid URL: " + url);
        const b = new URL(base);
        let path;
        const rel = /^([^?#]*)(\?[^#]*)?(#.*)?$/.exec(url);
        if (url.startsWith("//")) return new URL(b.protocol + url);
        if (rel[1] === "") path = b._path;
        else if (rel[1].startsWith("/")) path = rel[1];
        else path = b._path.replace(/[^/]*$/, "") + rel[1];
        const segs = [];
        for (const s of path.split("/")) {
          if (s === "..") { if (segs.length > 1) segs.pop(); } else if (s !== ".") segs.push(s);
        }
        if (/\/\.\.?$/.test(path)) segs.push("");
        m = [null, b.protocol, b._host !== null ? "//" : undefined, b._host, segs.join("/") || "/", rel[1] === "" && rel[2] === undefined ? b._search : rel[2], rel[3]];
      }
      this.protocol = m[1].toLowerCase();
      const auth = m[3] ?? null;
      this._host = auth;
      let hostport = auth || "";
      const at = hostport.lastIndexOf("@");
      this.username = ""; this.password = "";
      if (at >= 0) {
        const [u, p = ""] = hostport.slice(0, at).split(":");
        this.username = u; this.password = p;
        hostport = hostport.slice(at + 1);
      }
      const pm = /^(.*?)(?::(\d*))?$/.exec(hostport);
      this.hostname = pm[1].toLowerCase();
      this.port = pm[2] && pm[2] !== DEFAULT_PORTS[this.protocol] ? pm[2] : "";
      this._path = m[4] || (auth !== null ? "/" : "");
      this._search = m[5] && m[5] !== "?" ? m[5] : "";
      this.hash = m[6] && m[6] !== "#" ? m[6] : "";
      this.searchParams = new URLSearchParams(this._search);
      this.searchParams._url = this;
    }
    get pathname() { return this._path; }
    set pathname(v) { this._path = v.startsWith("/") ? v : "/" + v; }
    get search() { return this._search; }
    set search(v) {
      v = String(v);
      this._search = v && v !== "?" ? (v.startsWith("?") ? v : "?" + v) : "";
      this.searchParams._list = new URLSearchParams(this._search)._list;
    }
    get host() { return this.hostname + (this.port ? ":" + this.port : ""); }
    get origin() { return this._host !== null && this.protocol !== "file:" ? this.protocol + "//" + this.host : "null"; }
    get href() {
      const cred = this.username ? this.username + (this.password ? ":" + this.password : "") + "@" : "";
      return this.protocol + (this._host !== null ? "//" + cred + this.host : "") + this._path + this._search + this.hash;
    }
    toString() { return this.href; }
    toJSON() { return this.href; }
    static canParse(u, b) { try { new URL(u, b); return true; } catch { return false; } }
  };

  // structuredClone
  g.structuredClone = (value) => {
    const seen = new Map();
    const copy = (v) => {
      if (typeof v === "function" || typeof v === "symbol") throw new Error("DataCloneError: value could not be cloned");
      if (v === null || typeof v !== "object") return v;
      if (seen.has(v)) return seen.get(v);
      let out;
      if (v instanceof Date) return new Date(v.getTime());
      if (v instanceof RegExp) return new RegExp(v.source, v.flags);
      if (Array.isArray(v)) { out = []; seen.set(v, out); v.forEach((x, i) => { out[i] = copy(x); }); return out; }
      if (v instanceof Map) { out = new Map(); seen.set(v, out); v.forEach((x, k) => out.set(copy(k), copy(x))); return out; }
      if (v instanceof Set) { out = new Set(); seen.set(v, out); v.forEach((x) => out.add(copy(x))); return out; }
      if (ArrayBuffer.isView(v)) return new v.constructor(v);
      if (v instanceof ArrayBuffer) return v.slice(0);
      if (v instanceof Error) { out = new v.constructor(v.message); seen.set(v, out); return out; }
      out = {};
      seen.set(v, out);
      for (const k of Object.keys(v)) out[k] = copy(v[k]);
      return out;
    };
    return copy(value);
  };

  // matchMedia: width/height, orientation and colour-scheme queries against the window.
  g.matchMedia = (query) => {
    const text = String(query).toLowerCase();
    const evaluate = () => {
      const w = g.innerWidth, h = g.innerHeight;
      const px = (s) => { const m = /^(-?[\d.]+)(px|em|rem)?$/.exec(s.trim()); return m ? parseFloat(m[1]) * (m[2] === "em" || m[2] === "rem" ? 16 : 1) : NaN; };
      return text.split(",").some((alt) => {
        const parts = alt.replace(/^\s*(only\s+)?/, "").split(/\band\b/).map((s) => s.trim()).filter(Boolean);
        let negate = false;
        return parts.every((part, i) => {
          if (i === 0 && part === "not") { negate = true; return true; }
          if (part === "all" || part === "screen") return !negate || false;
          if (part === "print") return false;
          const m = /^\(\s*([a-z-]+)\s*(?::\s*([^)]*))?\)$/.exec(part);
          if (!m) return false;
          const [, f, val] = m;
          switch (f) {
            case "min-width": return w >= px(val);
            case "max-width": return w <= px(val);
            case "min-height": return h >= px(val);
            case "max-height": return h <= px(val);
            case "width": return w === px(val);
            case "height": return h === px(val);
            case "orientation": return (h >= w ? "portrait" : "landscape") === val.trim();
            case "prefers-color-scheme": return val.trim() === "light";
            case "prefers-reduced-motion": return val.trim() === "no-preference";
            case "hover": return val === undefined || val.trim() === "hover";
            case "pointer": return val === undefined || val.trim() === "fine";
            case "min-resolution": return (g.devicePixelRatio || 1) >= parseFloat(val);
            default: return false;
          }
        });
      });
    };
    const mql = new EventTarget();
    Object.defineProperties(mql, {
      media: { value: String(query) },
      matches: { get: evaluate },
    });
    mql.onchange = null;
    mql.addListener = (f) => mql.addEventListener("change", f);
    mql.removeListener = (f) => mql.removeEventListener("change", f);
    return mql;
  };

  // Web Storage. In memory for the life of the page; `sessionStorage` and `localStorage` are alike.
  class Storage {
    constructor() { Object.defineProperty(this, "_m", { value: new Map() }); }
    get length() { return this._m.size; }
    key(i) { return [...this._m.keys()][i] ?? null; }
    getItem(k) { k = String(k); return this._m.has(k) ? this._m.get(k) : null; }
    setItem(k, v) { this._m.set(String(k), String(v)); this._changed(); }
    removeItem(k) { if (this._m.delete(String(k))) this._changed(); }
    clear() { if (this._m.size) { this._m.clear(); this._changed(); } }
    _changed() {}
  }
  const storageProxy = (s) => new Proxy(s, {
    get: (t, k) => (k in t || typeof k === "symbol" ? t[k] : t.getItem(k) ?? undefined),
    set: (t, k, v) => { t.setItem(k, v); return true; },
    deleteProperty: (t, k) => { t.removeItem(k); return true; },
    has: (t, k) => k in t || t._m.has(k),
    ownKeys: (t) => [...t._m.keys()],
    getOwnPropertyDescriptor: (t, k) => (t._m.has(k) ? { value: t._m.get(k), writable: true, enumerable: true, configurable: true } : undefined),
  });
  g.Storage = Storage;
  Storage.open = (...a) => g.__storageModule.open(...a);
  const localRaw = new Storage();
  // `localStorage` is saved to the file the host chose (`ScriptHost::set_storage_file`), soon
  // after a change; without one it lives and dies with the page.
  let saveTimer = null;
  const save = () => { saveTimer = null; N.storageSave(JSON.stringify(Object.fromEntries(localRaw._m))); };
  localRaw._changed = () => { if (saveTimer === null) saveTimer = setTimeout(save, 50); };
  Object.defineProperty(g, "__storageLoad", {
    value(json) {
      try { for (const [k, v] of Object.entries(JSON.parse(json))) localRaw._m.set(k, String(v)); } catch { /* a damaged file starts empty */ }
    },
  });
  Object.defineProperty(g, "__storageFlush", { value() { if (saveTimer !== null) { clearTimeout(saveTimer); save(); } } });
  g.localStorage = storageProxy(localRaw);
  g.sessionStorage = storageProxy(new Storage());

  // MutationObserver: every DOM change goes through a native in `N`, so wrap those.
  const observers = new Set();
  let pending = false;
  const deliver = () => {
    pending = false;
    for (const o of [...observers]) {
      const records = o._queue.splice(0);
      if (records.length) { try { o._cb(records, o); } catch (e) { report(e); } }
    }
  };
  const within = (root, node) => { for (let n = node; n; n = n.parentNode) if (n === root) return true; return false; };
  const record = (type, target, extra) => {
    if (!observers.size || !target) return;
    for (const o of observers) {
      for (const [root, opt] of o._targets) {
        const ok = type === "childList" ? opt.childList : type === "attributes" ? opt.attributes && (!opt.attributeFilter || opt.attributeFilter.includes(extra.attributeName)) : opt.characterData;
        if (ok && (target === root || (opt.subtree && within(root, target)))) {
          o._queue.push({ type, target, addedNodes: [], removedNodes: [], previousSibling: null, nextSibling: null, attributeName: null, oldValue: null, ...extra });
          break;
        }
      }
    }
    if (!pending) { pending = true; Promise.resolve().then(deliver); }
  };
  if (N && typeof N.append === "function") {
    const wrapN = (name, fn) => {
      const orig = N[name];
      N[name] = function (...a) { return fn(orig.bind(N), ...a); };
    };
    const live = () => observers.size > 0;
    wrapN("append", (orig, p, c) => {
      const from = live() ? N.parent(c) : -1;
      const r = orig(p, c);
      if (live()) {
        if (from >= 0 && from !== p) record("childList", wrap(from), { removedNodes: [wrap(c)] });
        record("childList", wrap(p), { addedNodes: [wrap(c)] });
      }
      return r;
    });
    wrapN("insertBefore", (orig, p, c, ref) => {
      const from = live() ? N.parent(c) : -1;
      const r = orig(p, c, ref);
      if (live()) {
        if (from >= 0 && from !== p) record("childList", wrap(from), { removedNodes: [wrap(c)] });
        record("childList", wrap(p), { addedNodes: [wrap(c)] });
      }
      return r;
    });
    wrapN("remove", (orig, id) => {
      const from = live() ? N.parent(id) : -1;
      const r = orig(id);
      if (from >= 0) record("childList", wrap(from), { removedNodes: [wrap(id)] });
      return r;
    });
    wrapN("setAttr", (orig, id, name, value) => {
      const old = live() ? N.getAttr(id, name) : null;
      const r = orig(id, name, value);
      if (live() && old !== value) record("attributes", wrap(id), { attributeName: name, oldValue: old ?? null });
      return r;
    });
    wrapN("removeAttr", (orig, id, name) => {
      const old = live() ? N.getAttr(id, name) : null;
      const r = orig(id, name);
      if (live() && old != null) record("attributes", wrap(id), { attributeName: name, oldValue: old });
      return r;
    });
    wrapN("setText", (orig, id, text) => {
      const kids = live() ? N.childNodes(id).map(wrap) : [];
      const isText = live() && N.nodeType(id) === 3;
      const old = isText ? N.text(id) : null;
      const r = orig(id, text);
      if (isText) record("characterData", wrap(id), { oldValue: old });
      else if (live()) record("childList", wrap(id), { removedNodes: kids, addedNodes: N.childNodes(id).map(wrap) });
      return r;
    });
    wrapN("setInnerHTML", (orig, id, html) => {
      const kids = live() ? N.childNodes(id).map(wrap) : [];
      const r = orig(id, html);
      if (live()) record("childList", wrap(id), { removedNodes: kids, addedNodes: N.childNodes(id).map(wrap) });
      return r;
    });
  }
  g.MutationObserver = class MutationObserver {
    constructor(cb) {
      if (typeof cb !== "function") throw new TypeError("MutationObserver needs a callback");
      this._cb = cb; this._queue = []; this._targets = new Map();
    }
    observe(target, options = {}) {
      const opt = { ...options };
      if (opt.attributeFilter || opt.attributeOldValue) opt.attributes = opt.attributes ?? true;
      if (opt.characterDataOldValue) opt.characterData = opt.characterData ?? true;
      if (!opt.childList && !opt.attributes && !opt.characterData) throw new TypeError("The options must request childList, attributes or characterData");
      this._targets.set(target, opt);
      observers.add(this);
    }
    disconnect() { this._targets.clear(); this._queue.length = 0; observers.delete(this); }
    takeRecords() { return this._queue.splice(0); }
  };

  // ---- Blob, FormData, ReadableStream and friends --------------------------------
  const partBytes = (p) => (p instanceof Blob ? p._data() : p instanceof ArrayBuffer ? new Uint8Array(p) : ArrayBuffer.isView(p) ? new Uint8Array(p.buffer, p.byteOffset, p.byteLength) : new TextEncoder().encode(String(p)));
  g.Blob = class Blob {
    constructor(parts = [], opts = {}) {
      const list = Array.from(parts, partBytes);
      let n = 0;
      for (const b of list) n += b.length;
      const out = new Uint8Array(n);
      let at = 0;
      for (const b of list) { out.set(b, at); at += b.length; }
      Object.defineProperty(this, "_b", { value: out, writable: true });
      this.type = opts && opts.type ? String(opts.type).toLowerCase() : "";
    }
    _data() { return this._b; }
    get size() { return this._data().length; }
    slice(start = 0, end, type = "") { return new Blob([this._data().slice(start, end)], { type }); }
    text() { return Promise.resolve(new TextDecoder().decode(this._data())); }
    arrayBuffer() { const d = this._data(); return Promise.resolve(d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength)); }
    bytes() { return Promise.resolve(this._data().slice()); }
    stream() { const d = this._data().slice(); return new ReadableStream({ start(c) { if (d.length) c.enqueue(d); c.close(); } }); }
    get [Symbol.toStringTag]() { return "Blob"; }
  };
  // A queue of chunks with readers waiting on it (the part of the WHATWG streams that scripts use).
  g.ReadableStream = class ReadableStream {
    constructor(source = {}) {
      const s = this;
      Object.defineProperties(s, { _q: { value: [] }, _w: { value: [] }, _src: { value: source }, _st: { value: { done: false, err: undefined, failed: false, pulling: false, locked: false } } });
      s._ctrl = {
        enqueue(v) { if (s._st.done) return; const w = s._w.shift(); if (w) w.resolve({ value: v, done: false }); else s._q.push(v); },
        close() { if (s._st.done) return; s._st.done = true; for (const w of s._w.splice(0)) w.resolve({ value: undefined, done: true }); },
        error(e) { if (s._st.done) return; s._st.done = true; s._st.failed = true; s._st.err = e; for (const w of s._w.splice(0)) w.reject(e); },
        get desiredSize() { return 1 - s._q.length; },
      };
      Promise.resolve().then(() => source.start && source.start(s._ctrl)).then(() => s._pull(), (e) => s._ctrl.error(e));
    }
    _pull() {
      const st = this._st;
      if (!this._src.pull || st.pulling || st.done || (this._q.length >= 1 && !this._w.length)) return;
      st.pulling = true;
      Promise.resolve(this._src.pull(this._ctrl)).then(() => { st.pulling = false; if (this._w.length) this._pull(); }, (e) => { st.pulling = false; this._ctrl.error(e); });
    }
    _read() {
      if (this._q.length) { const v = this._q.shift(); this._pull(); return Promise.resolve({ value: v, done: false }); }
      if (this._st.failed) return Promise.reject(this._st.err);
      if (this._st.done) return Promise.resolve({ value: undefined, done: true });
      return new Promise((resolve, reject) => { this._w.push({ resolve, reject }); this._pull(); });
    }
    get locked() { return this._st.locked; }
    getReader() {
      if (this._st.locked) throw new TypeError("the stream is locked");
      this._st.locked = true;
      const s = this;
      return { read: () => s._read(), releaseLock() { s._st.locked = false; }, cancel: (r) => s.cancel(r), get closed() { return new Promise(() => {}); } };
    }
    cancel(reason) { this._q.length = 0; if (!this._st.done) { this._ctrl.close(); if (this._src.cancel) return Promise.resolve(this._src.cancel(reason)).then(() => undefined); } return Promise.resolve(); }
    async pipeTo(dest, opts = {}) {
      const w = dest.getWriter();
      try { for (;;) { const { value, done } = await this._read(); if (done) break; await w.write(value); } if (!opts.preventClose) await w.close(); }
      catch (e) { if (!opts.preventAbort) await w.abort(e); throw e; }
      finally { w.releaseLock(); }
    }
    pipeThrough(pair, opts) { this.pipeTo(pair.writable, opts).catch(() => {}); return pair.readable; }
    tee() {
      const chunks = [];
      let fin = false, failure;
      const waiters = [];
      const pump = (async () => { try { for (;;) { const { value, done } = await this._read(); if (done) break; chunks.push(value); waiters.splice(0).forEach((f) => f()); } } catch (e) { failure = e; } fin = true; waiters.splice(0).forEach((f) => f()); })();
      const branch = () => { let i = 0; return new ReadableStream({ async pull(c) { while (i >= chunks.length && !fin) await new Promise((r) => waiters.push(r)); if (i < chunks.length) c.enqueue(chunks[i++]); else if (failure) c.error(failure); else c.close(); } }); };
      void pump;
      return [branch(), branch()];
    }
    async *[Symbol.asyncIterator]() { for (;;) { const { value, done } = await this._read(); if (done) return; yield value; } }
    static from(iterable) {
      const it = iterable[Symbol.asyncIterator] ? iterable[Symbol.asyncIterator]() : iterable[Symbol.iterator]();
      return new ReadableStream({ async pull(c) { const r = await it.next(); if (r.done) c.close(); else c.enqueue(await r.value); } });
    }
  };
  g.WritableStream = class WritableStream {
    constructor(sink = {}) {
      this._sink = sink; this._locked = false; this._chain = Promise.resolve();
      this._ctrl = { error: (e) => { this._failed = e; } };
      this._chain = Promise.resolve().then(() => sink.start && sink.start(this._ctrl));
    }
    get locked() { return this._locked; }
    getWriter() {
      if (this._locked) throw new TypeError("the stream is locked");
      this._locked = true;
      const s = this;
      const queue = (fn) => (s._chain = s._chain.then(() => { if (s._failed) throw s._failed; return fn(); }));
      return {
        write: (chunk) => queue(() => s._sink.write && s._sink.write(chunk, s._ctrl)),
        close: () => queue(() => s._sink.close && s._sink.close()),
        abort: (r) => queue(() => s._sink.abort && s._sink.abort(r)).catch(() => {}),
        releaseLock() { s._locked = false; },
        get ready() { return Promise.resolve(); },
        get closed() { return s._chain; },
        desiredSize: 1,
      };
    }
    abort(r) { return this.getWriter().abort(r); }
    close() { return this.getWriter().close(); }
  };
  g.TransformStream = class TransformStream {
    constructor(t = {}) {
      let ctrl;
      this.readable = new ReadableStream({ start(c) { ctrl = c; } });
      const tc = { enqueue: (v) => ctrl.enqueue(v), error: (e) => ctrl.error(e), terminate: () => ctrl.close() };
      this.writable = new WritableStream({
        start: () => t.start && t.start(tc),
        write: (chunk) => (t.transform ? t.transform(chunk, tc) : ctrl.enqueue(chunk)),
        close: async () => { if (t.flush) await t.flush(tc); ctrl.close(); },
        abort: (e) => ctrl.error(e),
      });
    }
  };
  g.TextEncoderStream = class TextEncoderStream extends TransformStream {
    constructor() { const enc = new TextEncoder(); super({ transform(chunk, c) { c.enqueue(enc.encode(String(chunk))); } }); this.encoding = "utf-8"; }
  };
  g.TextDecoderStream = class TextDecoderStream extends TransformStream {
    constructor(label = "utf-8") {
      const dec = new TextDecoder(label); const parts = [];
      super({
        transform(chunk, c) { parts.push(...chunk); const text = dec.decode(Uint8Array.from(parts.splice(0))); if (text) c.enqueue(text); },
      });
      this.encoding = label;
    }
  };
  const formKinds = new Set(["button", "submit", "reset", "image"]);
  g.FormData = class FormData {
    constructor(form) {
      this._l = [];
      if (form && typeof form.querySelectorAll === "function") {
        for (const el of form.querySelectorAll("input, select, textarea")) {
          const name = el.getAttribute("name");
          if (!name || el.disabled) continue;
          const tag = el.localName, type = (el.getAttribute("type") || "text").toLowerCase();
          if (tag === "input") {
            if (formKinds.has(type)) continue;
            if ((type === "checkbox" || type === "radio") && !el.checked) continue;
            if (type === "file") { for (const f of el.files || []) this.append(name, f); continue; }
            this.append(name, type === "checkbox" || type === "radio" ? el.getAttribute("value") ?? "on" : el.value);
          } else if (tag === "select") {
            for (const o of el.querySelectorAll("option")) if (o.selected && !o.disabled) this.append(name, o.getAttribute("value") ?? o.textContent);
          } else this.append(name, el.value);
        }
      }
    }
    _entry(value, filename) { return value instanceof Blob ? (value instanceof File && filename === undefined ? value : new File([value], filename !== undefined ? String(filename) : value.name || "blob", { type: value.type })) : String(value); }
    append(name, value, filename) { this._l.push([String(name), this._entry(value, filename)]); }
    set(name, value, filename) {
      name = String(name);
      const i = this._l.findIndex((e) => e[0] === name);
      const entry = [name, this._entry(value, filename)];
      if (i < 0) this._l.push(entry); else { this._l[i] = entry; this._l = this._l.filter((e, k) => k <= i || e[0] !== name); }
    }
    get(name) { const e = this._l.find((e) => e[0] === String(name)); return e ? e[1] : null; }
    getAll(name) { return this._l.filter((e) => e[0] === String(name)).map((e) => e[1]); }
    has(name) { return this._l.some((e) => e[0] === String(name)); }
    delete(name) { this._l = this._l.filter((e) => e[0] !== String(name)); }
    forEach(fn, self) { for (const [k, v] of this._l) fn.call(self, v, k, this); }
    *entries() { for (const [k, v] of this._l) yield [k, v]; }
    *keys() { for (const [k] of this._l) yield k; }
    *values() { for (const [, v] of this._l) yield v; }
    [Symbol.iterator]() { return this.entries(); }
  };
  // `multipart/form-data`: the encoded bytes and the content type (with its boundary).
  const encodeMultipart = (fd) => {
    const boundary = "----NileFormBoundary" + Math.random().toString(16).slice(2).padEnd(14, "0") + Date.now().toString(16);
    const enc = new TextEncoder();
    const quote = (s) => String(s).replace(/"/g, "%22").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
    const parts = [];
    for (const [k, v] of fd._l) {
      if (v instanceof Blob) {
        parts.push(enc.encode(`--${boundary}\r\nContent-Disposition: form-data; name="${quote(k)}"; filename="${quote(v.name || "blob")}"\r\nContent-Type: ${v.type || "application/octet-stream"}\r\n\r\n`), v._data(), enc.encode("\r\n"));
      } else parts.push(enc.encode(`--${boundary}\r\nContent-Disposition: form-data; name="${quote(k)}"\r\n\r\n${v}\r\n`));
    }
    parts.push(enc.encode(`--${boundary}--\r\n`));
    return { bytes: partBytes(new Blob(parts)), type: "multipart/form-data; boundary=" + boundary };
  };
  const decodeMultipart = (bytes, boundary) => {
    const fd = new FormData();
    const text = Array.from(bytes, (b) => String.fromCharCode(b)).join(""); // latin-1: byte offsets stay character offsets
    const parts = text.split("--" + boundary);
    for (const part of parts.slice(1)) {
      if (part.startsWith("--")) break;
      const cut = part.indexOf("\r\n\r\n");
      if (cut < 0) continue;
      const head = part.slice(0, cut), body = part.slice(cut + 4, part.endsWith("\r\n") ? -2 : undefined);
      const name = /name="([^"]*)"/.exec(head), file = /filename="([^"]*)"/.exec(head), type = /content-type:\s*([^\r\n]+)/i.exec(head);
      if (!name) continue;
      const raw = Uint8Array.from(body, (c) => c.charCodeAt(0));
      if (file) fd.append(name[1], new File([raw], file[1], { type: type ? type[1] : "" }));
      else fd.append(name[1], new TextDecoder().decode(raw));
    }
    return fd;
  };

  // ---- fetch, Headers, Request, Response, AbortController, XMLHttpRequest ------
  const bytesToB64 = (u8) => {
    let bin = "";
    for (let i = 0; i < u8.length; i += 8192) bin += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
    return g.btoa(bin);
  };
  const b64ToBytes = (b64) => {
    const bin = g.atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  };
  g.Headers = class Headers {
    constructor(init) {
      this._m = new Map();
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (Array.isArray(init)) for (const [k, v] of init) this.append(k, v);
      else if (init) for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) { k = String(k).toLowerCase(); this._m.set(k, this._m.has(k) ? this._m.get(k) + ", " + v : String(v)); }
    set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
    get(k) { const v = this._m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
    has(k) { return this._m.has(String(k).toLowerCase()); }
    delete(k) { this._m.delete(String(k).toLowerCase()); }
    forEach(fn, self) { for (const [k, v] of this._m) fn.call(self, v, k, this); }
    keys() { return this._m.keys(); }
    values() { return this._m.values(); }
    entries() { return this._m.entries(); }
    [Symbol.iterator]() { return this._m.entries(); }
  };
  // The bytes of a request or response body from whatever the script gave.
  const bodyBytes = (b) => {
    if (b == null) return new Uint8Array(0);
    if (typeof b === "string") return new TextEncoder().encode(b);
    if (b instanceof URLSearchParams) return new TextEncoder().encode(b.toString());
    if (b instanceof Blob) return b._data();
    if (b instanceof FormData) return encodeMultipart(b).bytes;
    if (b instanceof ArrayBuffer) return new Uint8Array(b);
    if (ArrayBuffer.isView(b)) return new Uint8Array(b.buffer, b.byteOffset, b.byteLength);
    return new TextEncoder().encode(String(b));
  };
  class Body {
    _init(body) { this._bytes = bodyBytes(body); this.bodyUsed = false; }
    _take() {
      if (this.bodyUsed) return Promise.reject(new TypeError("body stream already read"));
      this.bodyUsed = true;
      return Promise.resolve(this._bytes);
    }
    text() { return this._take().then((b) => new TextDecoder().decode(b)); }
    json() { return this.text().then((t) => JSON.parse(t)); }
    arrayBuffer() { return this._take().then((b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)); }
    bytes() { return this._take(); }
    blob() { return this._take().then((b) => new Blob([b], { type: (this.headers && this.headers.get("content-type")) || "" })); }
    formData() {
      const type = (this.headers && this.headers.get("content-type")) || "";
      return this._take().then((b) => {
        if (/multipart\/form-data/i.test(type)) { const m = /boundary=("?)([^";]+)\1/i.exec(type); if (m) return decodeMultipart(b, m[2]); }
        if (/x-www-form-urlencoded/i.test(type)) { const fd = new FormData(); for (const [k, v] of new URLSearchParams(new TextDecoder().decode(b))) fd.append(k, v); return fd; }
        throw new TypeError("the body is not form data");
      });
    }
    get body() { return this._bytes.length || !this.bodyUsed ? new Blob([this._bytes]).stream() : null; }
  }
  g.Request = class Request extends Body {
    constructor(input, init = {}) {
      super();
      const from = input instanceof Request ? input : null;
      this.url = from ? from.url : String(input);
      this.method = String(init.method || (from && from.method) || "GET").toUpperCase();
      this.headers = new Headers(init.headers || (from && from.headers));
      this.signal = init.signal || (from && from.signal) || null;
      const multi = init.body instanceof FormData ? encodeMultipart(init.body) : null;
      this._init(multi ? multi.bytes : init.body !== undefined ? init.body : from && from._bytes);
      if (multi && !this.headers.has("content-type")) this.headers.set("content-type", multi.type);
      if (init.body instanceof Blob && init.body.type && !this.headers.has("content-type")) this.headers.set("content-type", init.body.type);
      if (typeof init.body === "string" && !this.headers.has("content-type")) this.headers.set("content-type", "text/plain;charset=UTF-8");
      if (init.body instanceof URLSearchParams && !this.headers.has("content-type")) this.headers.set("content-type", "application/x-www-form-urlencoded;charset=UTF-8");
    }
    clone() { return new Request(this); }
  };
  g.Response = class Response extends Body {
    constructor(body, init = {}) {
      super();
      this._init(body);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText || "";
      this.headers = new Headers(init.headers);
      this.url = init.url || "";
      this.type = "basic";
    }
    get ok() { return this.status >= 200 && this.status < 300; }
    clone() { return new Response(this._bytes, this); }
    static json(data, init = {}) { const r = new Response(JSON.stringify(data), init); r.headers.set("content-type", "application/json"); return r; }
    static error() { const r = new Response(null, { status: 0 }); r.type = "error"; return r; }
  };
  g.AbortSignal = class AbortSignal extends EventTarget {
    constructor() { super(); this.aborted = false; this.reason = undefined; this.onabort = null; }
    throwIfAborted() { if (this.aborted) throw this.reason; }
    static abort(reason) { const s = new AbortSignal(); s._abort(reason); return s; }
    _abort(reason) {
      if (this.aborted) return;
      this.aborted = true;
      this.reason = reason === undefined ? new Error("AbortError: the operation was aborted") : reason;
      const ev = new Event("abort");
      this.dispatchEvent(ev);
    }
  };
  g.AbortController = class AbortController {
    constructor() { this.signal = new AbortSignal(); }
    abort(reason) { this.signal._abort(reason); }
  };
  const inflight = new Map();
  let nextFetch = 1;
  const resolveUrl = (u) => { try { return new URL(u, g.location && g.location.href && g.location.href !== "about:blank" ? g.location.href : undefined).href; } catch { return u; } };
  g.fetch = (input, init) => new Promise((resolve, reject) => {
    const req = new Request(input, init);
    if (req.signal && req.signal.aborted) return reject(req.signal.reason);
    const id = nextFetch++;
    inflight.set(id, { resolve, reject, req });
    if (req.signal) req.signal.addEventListener("abort", () => {
      if (inflight.delete(id)) reject(req.signal.reason);
    });
    const flat = [];
    req.headers.forEach((v, k) => flat.push(k, v));
    N.fetchStart(id, req.method, req.url, flat, bytesToB64(req._bytes));
  });
  Object.defineProperty(g, "__fetchDone", {
    value(id, status, statusText, url, headers, body, error) {
      const f = inflight.get(id);
      if (!f) return; // aborted
      inflight.delete(id);
      if (error) return f.reject(new TypeError("Failed to fetch: " + error));
      const h = {};
      for (let i = 0; i + 1 < headers.length; i += 2) h[headers[i]] = headers[i + 1];
      f.resolve(new Response(b64ToBytes(body), { status, statusText, headers: h, url: url || f.req.url }));
    },
  });

  g.XMLHttpRequest = class XMLHttpRequest extends EventTarget {
    constructor() {
      super();
      this.readyState = 0; this.status = 0; this.statusText = ""; this.response = null; this.responseText = "";
      this.responseType = ""; this.responseURL = ""; this.timeout = 0; this.withCredentials = false;
      this._headers = new Headers(); this._resp = new Headers();
      for (const k of ["onreadystatechange", "onload", "onerror", "onabort", "ontimeout", "onloadend", "onloadstart", "onprogress"]) this[k] = null;
    }
    _fire(type) {
      const ev = new Event(type);
      this.dispatchEvent(ev);
    }
    _state(n) { this.readyState = n; this._fire("readystatechange"); }
    open(method, url) { this._method = String(method).toUpperCase(); this._url = String(url); this._state(1); }
    setRequestHeader(k, v) { this._headers.append(k, v); }
    getResponseHeader(k) { return this._resp.get(k); }
    getAllResponseHeaders() { let s = ""; this._resp.forEach((v, k) => { s += k + ": " + v + "\r\n"; }); return s; }
    overrideMimeType() {}
    abort() {
      if (this._ctl) { this._ctl.abort(); this._ctl = null; this.readyState = 0; this._fire("abort"); this._fire("loadend"); }
    }
    send(body = null) {
      this._ctl = new AbortController();
      this._fire("loadstart");
      let timer = null, timedOut = false;
      if (this.timeout > 0) timer = setTimeout(() => { timedOut = true; this._ctl.abort(); }, this.timeout);
      fetch(this._url, { method: this._method, headers: this._headers, body: this._method === "GET" || this._method === "HEAD" ? undefined : body, signal: this._ctl.signal })
        .then((r) => {
          this.status = r.status; this.statusText = r.statusText; this._resp = r.headers; this.responseURL = r.url;
          this._state(2);
          return r.arrayBuffer().then((buf) => {
            this._state(3);
            const text = () => new TextDecoder().decode(new Uint8Array(buf));
            if (this.responseType === "arraybuffer") this.response = buf;
            else if (this.responseType === "json") { try { this.response = JSON.parse(text()); } catch { this.response = null; } }
            else { this.responseText = text(); this.response = this.responseText; }
            this._ctl = null;
            this._state(4);
            this._fire("load");
            this._fire("loadend");
          });
        })
        .catch((e) => {
          if (timer) clearTimeout(timer);
          if (!this._ctl) return; // aborted by the script
          this._ctl = null;
          this._state(4);
          this._fire(timedOut ? "timeout" : "error");
          this._fire("loadend");
        });
    }
  };
  Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });

  // ---- location, history, navigator --------------------------------------------
  let here = new URL("about:blank");
  Object.defineProperty(g, "__setLocation", { value(url) { try { here = new URL(url); } catch { here = new URL("about:blank"); } } });
  const hashChanged = (old) => {
    if (old !== here.href) {
      const ev = new Event("hashchange");
      ev.oldURL = old; ev.newURL = here.href;
      g.dispatchEvent(ev);
    }
  };
  const loc = {
    get href() { return here.href; },
    set href(v) {
      const old = here.href;
      const next = new URL(v, here.href);
      if (next.href.split("#")[0] === old.split("#")[0]) { here = next; hashChanged(old); } // other pages can't be opened from a script
    },
    get protocol() { return here.protocol; },
    get host() { return here.host; },
    get hostname() { return here.hostname; },
    get port() { return here.port; },
    get origin() { return here.origin; },
    get pathname() { return here.pathname; },
    get search() { return here.search; },
    get hash() { return here.hash; },
    set hash(v) {
      const old = here.href;
      here.hash = v ? (String(v).startsWith("#") ? String(v) : "#" + v) : "";
      hashChanged(old);
    },
    assign(u) { loc.href = u; },
    replace(u) { loc.href = u; },
    reload() {},
    toString() { return here.href; },
  };
  Object.defineProperty(g, "location", { get: () => loc, set: (v) => { loc.href = v; } });
  Object.defineProperty(document, "location", { get: () => loc });
  Object.defineProperty(document, "URL", { get: () => here.href });
  const states = [null];
  Object.defineProperty(g, "history", {
    value: {
      get length() { return states.length; },
      get state() { return states[states.length - 1]; },
      scrollRestoration: "auto",
      pushState(state, title, url) { states.push(state ?? null); if (url != null) here = new URL(url, here.href); },
      replaceState(state, title, url) { states[states.length - 1] = state ?? null; if (url != null) here = new URL(url, here.href); },
      back() {}, forward() {}, go() {},
    },
  });
  Object.defineProperty(g, "navigator", {
    value: { userAgent: "Mozilla/5.0 Nile/0.1", appName: "Nile", language: "en-US", languages: ["en-US", "en"], onLine: true, platform: "", hardwareConcurrency: 4, cookieEnabled: false, maxTouchPoints: 0 },
  });

  // ---- ResizeObserver, IntersectionObserver ------------------------------------
  // The host calls `__observeCheck` after layout changes and scrolls while any are active.
  const watchers = new Set();
  const refresh = () => N.observing(watchers.size > 0);
  const box = (x, y, w, h) => ({ x, y, width: w, height: h, top: y, left: x, right: x + w, bottom: y + h });
  g.ResizeObserver = class ResizeObserver {
    constructor(cb) { this._cb = cb; this._seen = new Map(); }
    observe(el) { if (!this._seen.has(el)) { this._seen.set(el, null); watchers.add(this); refresh(); } }
    unobserve(el) { this._seen.delete(el); if (!this._seen.size) { watchers.delete(this); refresh(); } }
    disconnect() { this._seen.clear(); watchers.delete(this); refresh(); }
    _check() {
      const entries = [];
      for (const [el, last] of this._seen) {
        const r = el.getBoundingClientRect();
        if (!last || last[0] !== r.width || last[1] !== r.height) {
          this._seen.set(el, [r.width, r.height]);
          const size = [{ inlineSize: r.width, blockSize: r.height }];
          entries.push({ target: el, contentRect: box(0, 0, r.width, r.height), borderBoxSize: size, contentBoxSize: size });
        }
      }
      if (entries.length) try { this._cb(entries, this); } catch (e) { report(e); }
    }
  };
  g.IntersectionObserver = class IntersectionObserver {
    constructor(cb, options = {}) {
      this._cb = cb; this.root = options.root || null; this._seen = new Map();
      const t = options.threshold === undefined ? [0] : [].concat(options.threshold);
      this.thresholds = t.slice().sort((a, b) => a - b);
    }
    observe(el) { if (!this._seen.has(el)) { this._seen.set(el, -2); watchers.add(this); refresh(); } }
    unobserve(el) { this._seen.delete(el); if (!this._seen.size) { watchers.delete(this); refresh(); } }
    disconnect() { this._seen.clear(); watchers.delete(this); refresh(); }
    takeRecords() { return []; }
    _check() {
      const rb = this.root ? this.root.getBoundingClientRect() : box(0, 0, g.innerWidth, g.innerHeight);
      const entries = [];
      for (const [el, last] of this._seen) {
        const r = el.getBoundingClientRect();
        const x0 = Math.max(r.left, rb.left), y0 = Math.max(r.top, rb.top);
        const x1 = Math.min(r.right, rb.right), y1 = Math.min(r.bottom, rb.bottom);
        const inter = x1 >= x0 && y1 >= y0;
        const area = r.width * r.height;
        const ratio = !inter ? 0 : area > 0 ? ((x1 - x0) * (y1 - y0)) / area : 1;
        // The index of the highest threshold reached (-1 below all of them, or not intersecting).
        let idx = -1;
        this.thresholds.forEach((t, i) => { if (inter && (ratio >= t && (t > 0 || inter))) idx = i; });
        if (idx !== last) {
          this._seen.set(el, idx);
          entries.push({
            target: el, isIntersecting: inter && idx >= 0, intersectionRatio: ratio, time: g.performance.now(),
            boundingClientRect: r, intersectionRect: inter ? box(x0, y0, x1 - x0, y1 - y0) : box(0, 0, 0, 0), rootBounds: rb,
          });
        }
      }
      if (entries.length) try { this._cb(entries, this); } catch (e) { report(e); }
    }
  };
  Object.defineProperty(g, "__observeCheck", { value() { for (const w of [...watchers]) w._check(); } });

  // ---- File objects for <input type=file> --------------------------------------
  const MIME = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp", svg: "image/svg+xml",
    txt: "text/plain", json: "application/json", html: "text/html", css: "text/css", js: "text/javascript", csv: "text/csv", pdf: "application/pdf" };
  // `new File(path)` is a file the user chose in `<input type=file>`; `new File(parts, name, options)` is a Blob with a name.
  g.File = class File extends Blob {
    constructor(a, name, opts = {}) {
      const path = typeof a === "string" && name === undefined;
      const fileName = path ? a.split(/[\\/]/).pop() : String(name);
      super(path ? [] : a, { type: opts.type || "" });
      this.name = fileName;
      this.lastModified = path ? 0 : opts.lastModified === undefined ? Date.now() : opts.lastModified;
      if (path) Object.defineProperty(this, "path", { value: a, enumerable: true });
      if (path && !this.type) this.type = MIME[(fileName.split(".").pop() || "").toLowerCase()] || "";
    }
    _data() {
      if (!this.path) return super._data();
      const r = N.fsCall("read", [this.path]);
      if (r.startsWith("E:")) throw new Error(r.slice(2));
      return b64ToBytes(r.slice(2));
    }
    get size() { return this.path ? Math.max(0, N.fileSize(this.path)) : super.size; }
    get [Symbol.toStringTag]() { return "File"; }
  };
  Object.defineProperty(Element.prototype, "files", {
    configurable: true,
    get() {
      if (this.localName !== "input" || this.type !== "file") return undefined;
      const list = (this.getAttribute("nile-files") || "").split("\n").filter(Boolean).map((p) => new File(p));
      list.item = (i) => list[i] || null;
      return list;
    },
  });

  // ---- the `@sciter` and `@env` modules (see docs.sciter.com/docs/JS.runtime) -----
  const toBytes = (b) => (b instanceof ArrayBuffer ? new Uint8Array(b) : ArrayBuffer.isView(b) ? new Uint8Array(b.buffer, b.byteOffset, b.byteLength) : new TextEncoder().encode(String(b)));
  const bufferOf = (u8) => u8.buffer.slice(u8.byteOffset, u8.byteOffset + u8.byteLength);
  const crcTable = (() => {
    const t = new Uint32Array(256);
    for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
    return t;
  })();
  const crc32 = (data) => {
    let c = 0xffffffff;
    for (const b of toBytes(data)) c = crcTable[(c ^ b) & 255] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const md5 = (data) => {
    const bytes = toBytes(data);
    const K = Array.from({ length: 64 }, (_, i) => Math.floor(Math.abs(Math.sin(i + 1)) * 4294967296) >>> 0);
    const S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    const total = ((bytes.length + 8) >> 6) + 1;
    const words = new Uint32Array(total * 16);
    for (let i = 0; i < bytes.length; i++) words[i >> 2] |= bytes[i] << ((i % 4) * 8);
    words[bytes.length >> 2] |= 0x80 << ((bytes.length % 4) * 8);
    words[total * 16 - 2] = (bytes.length * 8) >>> 0;
    words[total * 16 - 1] = Math.floor((bytes.length * 8) / 4294967296);
    let a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
    for (let o = 0; o < words.length; o += 16) {
      let a = a0, b = b0, c = c0, d = d0;
      for (let i = 0; i < 64; i++) {
        let f, gi;
        if (i < 16) { f = (b & c) | (~b & d); gi = i; }
        else if (i < 32) { f = (d & b) | (~d & c); gi = (5 * i + 1) % 16; }
        else if (i < 48) { f = b ^ c ^ d; gi = (3 * i + 5) % 16; }
        else { f = c ^ (b | ~d); gi = (7 * i) % 16; }
        const sum = (a + f + K[i] + words[o + gi]) | 0;
        const sh = S[(i >> 4) * 4 + (i % 4)];
        a = d; d = c; c = b;
        b = (b + ((sum << sh) | (sum >>> (32 - sh)))) | 0;
      }
      a0 = (a0 + a) | 0; b0 = (b0 + b) | 0; c0 = (c0 + c) | 0; d0 = (d0 + d) | 0;
    }
    return [a0, b0, c0, d0].map((w) => [0, 8, 16, 24].map((s) => ((w >>> s) & 255).toString(16).padStart(2, "0")).join("")).join("");
  };
  // "JSON++": JSON plus unquoted names and strings, `;` separators, hex numbers, bigints, dates.
  const parseValue = (text) => {
    const src = String(text);
    let i = 0;
    const ws = () => { while (i < src.length && /\s/.test(src[i])) i++; };
    const fail = () => { throw new SyntaxError("parseValue: unexpected input at " + i); };
    const word = () => { const m = /^[^\s,;:{}\[\]"']+/.exec(src.slice(i)); if (!m) fail(); i += m[0].length; return m[0]; };
    const value = () => {
      ws();
      const c = src[i];
      if (c === "{") {
        i++;
        const o = {};
        for (;;) {
          ws();
          if (src[i] === "}") { i++; return o; }
          const k = src[i] === '"' || src[i] === "'" ? str() : word();
          ws();
          if (src[i] !== ":") fail();
          i++;
          o[k] = value();
          ws();
          if (src[i] === "," || src[i] === ";") i++;
        }
      }
      if (c === "[") {
        i++;
        const a = [];
        for (;;) {
          ws();
          if (src[i] === "]") { i++; return a; }
          a.push(value());
          ws();
          if (src[i] === ",") i++;
        }
      }
      if (c === '"' || c === "'") return str();
      const w = word();
      if (w === "true") return true;
      if (w === "false") return false;
      if (w === "null") return null;
      if (w === "undefined") return undefined;
      if (/^-?(0x[0-9a-f]+|\d+\.?\d*(e[+-]?\d+)?)$/i.test(w)) return Number(w);
      if (/^-?\d+n$/.test(w)) return BigInt(w.slice(0, -1));
      const d = /^0d(\d{4})-(\d{2})-(\d{2})$/.exec(w);
      if (d) return new Date(Date.UTC(+d[1], d[2] - 1, +d[3]));
      return w; // other bare words (lengths like `12px`, names) stay text
    };
    const str = () => {
      const q = src[i++];
      let out = "";
      while (i < src.length && src[i] !== q) {
        if (src[i] === "\\") { i++; out += { n: "\n", t: "\t", r: "\r" }[src[i]] ?? src[i]; } else out += src[i];
        i++;
      }
      i++;
      return out;
    };
    const v = value();
    ws();
    if (i < src.length) fail();
    return v;
  };
  const LENGTH_UNITS = { px: 1, in: 96, cm: 96 / 2.54, mm: 96 / 25.4, pt: 96 / 72, pc: 16 };
  const unsupported = (what) => () => { throw new Error(what + " is not available in Nile"); };
  const sciterModule = {
    VERSION: "1.0.0.0", REVISION: "0", QUICKJS_VERSION: "2024-01-13",
    import: unsupported("import() (use `await import(...)`)"),
    setModuleUrlResolver() { /* bare names are not resolved yet */ },
    loadLibrary: unsupported("loadLibrary"),
    parseValue,
    devicePixels(len, axis) {
      const m = typeof len === "number" ? [0, len, "px"] : /^\s*(-?[\d.]+)\s*([a-z%]*)\s*$/i.exec(String(len)) || [0, 0, "px"];
      return Math.round(parseFloat(m[1]) * (LENGTH_UNITS[m[2] || "px"] ?? 1) * g.devicePixelRatio);
    },
    on: (...a) => document.on(...a),
    off: (...a) => document.off(...a),
    once: (...a) => document.once(...a),
    onGlobalEvent: (name, handler) => { g.addEventListener(name, handler); return g; },
    offGlobalEvent: (what) => { if (typeof what === "function") for (const n of []) g.removeEventListener(n, what); return g; },
    $: (sel) => document.querySelector(sel),
    $$: (sel) => Array.from(document.querySelectorAll(sel)),
    uuid() {
      const h = (n) => Array.from({ length: n }, () => Math.floor(Math.random() * 16).toString(16)).join("");
      return `${h(8)}-${h(4)}-4${h(3)}-${"89ab"[Math.floor(Math.random() * 4)]}${h(3)}-${h(12)}`;
    },
    encode: (text, enc = "utf-8") => bufferOf(/^(latin1|iso-8859-1|ascii|us-ascii)$/i.test(enc) ? Uint8Array.from(String(text), (c) => c.charCodeAt(0) & 255) : new TextEncoder().encode(text)),
    decode: (bytes, enc = "utf-8") => (/^(latin1|iso-8859-1|ascii|us-ascii)$/i.test(enc) ? String.fromCharCode(...toBytes(bytes)) : new TextDecoder().decode(toBytes(bytes))),
    compress: unsupported("compress"),
    decompress: unsupported("decompress"),
    toBase64: (buf) => bytesToB64(toBytes(buf)),
    fromBase64: (s) => bufferOf(b64ToBytes(String(s))),
    md5, crc32,
  };
  const call = (op, ...args) => N.envCall(op, args);
  const lines = (s) => (s ? s.split("\n") : []);
  const toUrl = (p) => "file://" + (p.startsWith("/") ? "" : "/") + p.replace(/\\/g, "/").split("/").map(encodeURIComponent).join("/").replace(/%3A/g, ":");
  const envModule = {
    get OS() { return call("os"); },
    get PLATFORM() { return call("platform"); },
    DEVICE: "desktop",
    language: () => call("language"),
    country: () => call("country"),
    userName: () => call("userName"),
    machineName: () => call("machineName"),
    domainName: () => call("domainName"),
    arguments: () => lines(call("arguments")),
    launch: (path) => { call("launch", String(path)); },
    home: (rel = "") => call("home", rel),
    homeURL: (rel = "") => toUrl(call("home", rel)),
    path: (name, rel = "") => call("path", name, rel),
    pathURL: (name, rel = "") => toUrl(call("path", name, rel)),
    drives: () => lines(call("drives")),
    variable: (name, set) => (set === undefined ? call("variable", String(name)) : call("variable", String(name), set === null ? "\u0000" : String(set))),
    exec: (...args) => { call("exec", ...args.map(String)); },
  };

  // ---- <frame> / <iframe> ------------------------------------------------------
  // The sub-document is a separate page with its own script context, so the outer page reaches
  // into it by running code there (`__frameEval`) and talks to it with messages.
  const isFrame = (el) => el.localName === "frame" || el.localName === "iframe";
  const rpc = (frame, code) => {
    const r = host.__frameEval(frame._id, "JSON.stringify(" + code + ")");
    if (!r || !r.ok) throw new Error((r && r.error) || "the frame has no document");
    return r.v === "" ? undefined : JSON.parse(r.v);
  };
  const lit = (v) => JSON.stringify(v === undefined ? null : v);
  class FrameElement {
    constructor(frame, expr) { this._f = frame; this._e = expr; }
    _get(body) { return rpc(this._f, `(function (e) { return e ? ${body} : undefined; })(${this._e})`); }
    _do(body) { rpc(this._f, `(function (e) { if (e) { ${body} } })(${this._e})`); }
    get exists() { return rpc(this._f, `!!(${this._e})`); }
    get tag() { return this._get("e.localName"); }
    get text() { return this._get("e.textContent"); }
    set text(v) { this._do(`e.textContent = ${lit(String(v))};`); }
    get html() { return this._get("e.innerHTML"); }
    set html(v) { this._do(`e.innerHTML = ${lit(String(v))};`); }
    get value() { return this._get("e.value"); }
    set value(v) { this._do(`e.value = ${lit(v)};`); }
    getAttribute(name) { return this._get(`e.getAttribute(${lit(String(name))})`) ?? null; }
    setAttribute(name, v) { this._do(`e.setAttribute(${lit(String(name))}, ${lit(String(v))});`); }
    click() { this._do("e.click();"); }
    $(sel) { const x = new FrameElement(this._f, `(${this._e}).querySelector(${lit(sel)})`); return x.exists ? x : null; }
    $$(sel) {
      const n = rpc(this._f, `(${this._e}).querySelectorAll(${lit(sel)}).length`);
      return Array.from({ length: n }, (_, i) => new FrameElement(this._f, `(${this._e}).querySelectorAll(${lit(sel)})[${i}]`));
    }
  }
  class FrameDocument {
    constructor(frame) { this._f = frame; }
    $(sel) { const x = new FrameElement(this._f, `document.querySelector(${lit(sel)})`); return x.exists ? x : null; }
    $$(sel) {
      const n = rpc(this._f, `document.querySelectorAll(${lit(sel)}).length`);
      return Array.from({ length: n }, (_, i) => new FrameElement(this._f, `document.querySelectorAll(${lit(sel)})[${i}]`));
    }
    querySelector(sel) { return this.$(sel); }
    querySelectorAll(sel) { return this.$$(sel); }
    eval(code) { return rpc(this._f, `(0, eval)(${lit(String(code))})`); }
    call(name, ...args) { return rpc(this._f, `${name}(...${lit(args)})`); }
    get title() { return rpc(this._f, "document.title"); }
    get body() { return this.$("body"); }
  }
  const frameProp = (name, desc) => Object.defineProperty(Element.prototype, name, { configurable: true, ...desc });
  frameProp("frame", { get() { return isFrame(this) ? this : undefined; } });
  frameProp("document", { get() { return isFrame(this) ? new FrameDocument(this) : undefined; } });
  frameProp("contentDocument", { get() { return isFrame(this) ? new FrameDocument(this) : undefined; } });
  frameProp("contentWindow", {
    get() {
      if (!isFrame(this)) return undefined;
      const f = this;
      return { postMessage: (data) => f.postMessage(data), document: new FrameDocument(f), eval: (code) => new FrameDocument(f).eval(code) };
    },
  });
  frameProp("url", {
    get() { return isFrame(this) ? this.getAttribute("src") || "" : undefined; },
    set(v) { if (isFrame(this)) this.loadFile(v); },
  });
  const frameMethod = (name, fn) => Object.defineProperty(Element.prototype, name, { configurable: true, writable: true, value: fn });
  frameMethod("loadFile", function (url) {
    if (!isFrame(this)) return false;
    this.removeAttribute("srcdoc");
    this.setAttribute("src", String(url));
    return true;
  });
  frameMethod("loadHtml", function (html, baseUrl) {
    if (!isFrame(this)) return false;
    if (baseUrl) this.setAttribute("nile-base", String(baseUrl).replace(/[^/\\]*$/, ""));
    this.setAttribute("srcdoc", typeof html === "string" ? html : new TextDecoder().decode(html));
    return true;
  });
  frameMethod("loadEmpty", function () { return isFrame(this) ? this.loadHtml("") : false; });
  frameMethod("postMessage", function (data) {
    if (isFrame(this)) host.__framePost(this._id, JSON.stringify(data) ?? "null");
  });
  // `frameset.state`: the panes' current sizes (widths for `cols`, heights for `rows`), settable.
  frameProp("frameset", {
    get() {
      if (this.localName !== "frameset") return undefined;
      const el = this;
      const panes = () => Array.from(el.children).filter((c) => c.localName !== "splitter");
      return {
        get state() {
          const wide = el.hasAttribute("cols");
          return panes().map((p) => { const r = p.getBoundingClientRect(); return wide ? r.width : r.height; });
        },
        set state(sizes) {
          el.setAttribute(el.hasAttribute("cols") ? "cols" : "rows", Array.from(sizes, (n) => n + "px").join(","));
        },
      };
    },
  });
  // Inside a frame, `window.parent` can post messages up; at the top it is the window itself.
  const parentWindow = { postMessage: (data) => { host.__parentPost(JSON.stringify(data) ?? "null"); } };
  const upward = { configurable: true, get() { return host.has("__parentPost") ? parentWindow : g; } };
  Object.defineProperty(g, "parent", upward);
  Object.defineProperty(g, "top", upward);
  Object.defineProperty(g, "__frameMessage", {
    value(...args) {
      // From the outer page: (frame node id, data). From a frame's parent: (data).
      const fromFrame = args.length === 2;
      const event = new Event("message");
      event.data = fromFrame ? args[1] : args[0];
      event.origin = "";
      event.source = fromFrame ? wrap(args[0]) : parentWindow;
      g.dispatchEvent(event);
    },
  });

  // ---- Window: native dialogs -----------------------------------------------------
  // Shapes follow docs.sciter.com/docs/DOM/Window. `Window.this` is this document's window and
  // `new Window({ url, ... })` opens another one (the shell creates it; see `WindowCommand::Open`).
  const dialog = (kind, a = "", b = "", c = "", d = "") => N.dialog(kind, String(a), String(b), String(c), String(d));
  const textOf = (n) => (Array.isArray(n) ? n.map(textOf).join("") : n == null || typeof n === "boolean" ? "" : typeof n === "object" ? "" : String(n));
  let appMenu = null;
  // Sciter's `application` (bootstrap scripts, docs.sciter.com scapp): the program's own loop is the shell's,
  // so `run()` returns at once and `quit(code)` ends the program with that exit code.
  Object.defineProperty(g, "application", { value: { start() {}, run() { return 0; }, quit(code = 0) { N.windowCommand("quit", +code | 0, 0); } }, configurable: true });
  // Classic scripts have no module scope: the page's folder.
  Object.defineProperty(g, "__DIR__", { get() { return String(location.href).replace(/[^/]*$/, ""); }, configurable: true });
  Object.defineProperty(g, "__FILE__", { get() { return String(location.href); }, configurable: true });
  class Window {
    constructor(params = {}) { openWindow(this, params, false); }
    static get this() { return thisWindow; }
    static get all() { return [thisWindow, ...[...known.values()].filter((w) => w._valid && w !== thisWindow)]; }
    static get share() { return shared; }
    // selectFile({ mode: "open" | "open-multiple" | "save", filter, extension, caption, path })
    selectFile(params = {}) {
      const mode = params.mode || "open";
      const answer = mode === "save"
        ? dialog("save", params.filter, params.extension, params.caption, params.path)
        : dialog("open", mode, params.filter, params.caption, params.path);
      if (answer === undefined || answer === "") return null;
      return mode === "open-multiple" ? lines(answer).map(toUrl) : toUrl(lines(answer)[0]);
    }
    selectFolder(params = {}) {
      const answer = dialog("folder", "", "", params.caption, params.path);
      return answer ? toUrl(answer) : null;
    }
    // modal(<alert>text</alert>) shows a message box (<info>, <alert>, <error>, <question>). By default it
    // is the system's own box and the answer comes back at once: "yes" (or "yes" from an OK) or, for a
    // question, "yes"/"no". `Window.this.messageBoxes = "inline"` (or an `inline` attribute on the tag)
    // draws it in the window instead, as styleable HTML with your own <button>s and rich content; that
    // one can't block the script, so it returns a Promise (`await` works for both). `native` forces the
    // system box. What Sciter returns isn't documented.
    modal(arg) {
      // modal({ url, ... }) opens a dialog window; it returns a Promise of what the dialog passes to
      // `window.close(value)` (the script can't block, unlike Sciter's).
      if (arg && !Array.isArray(arg) && typeof arg === "object") {
        const w = Object.create(Window.prototype);
        openWindow(w, { type: Window.DIALOG_WINDOW, ...arg }, true);
        return w.closed;
      }
      const vnode = Array.isArray(arg) && typeof arg[0] === "string" ? arg : Array.isArray(arg) ? arg.find((v) => Array.isArray(v)) : null;
      if (!vnode) throw new Error("Window.modal() supports the message boxes <info>, <alert>, <error> and <question> only");
      const [tag, attrs, kids] = vnode;
      if (!["info", "alert", "error", "question"].includes(tag)) throw new Error(`Window.modal(): unknown message box <${tag}>`);
      const caption = (attrs && (attrs.caption || attrs.title)) || document.title || "";
      const inline = attrs && attrs.native !== undefined ? false : attrs && attrs.inline !== undefined ? true : boxMode === "inline";
      if (inline) return inlineBox(tag, attrs || {}, kids || [], caption);
      const answer = dialog("message", tag, textOf(kids).trim(), caption);
      if (answer === undefined) return tag === "question" ? "no" : "yes";
      return tag === "question" ? answer : "yes";
    }
    // Show popups (`element.popup()`) in OS windows of their own, so they can extend past this window.
    get popupWindows() { return popupWindows; }
    set popupWindows(v) { popupWindows = !!v; }
    get messageBoxes() { return boxMode; }
    set messageBoxes(v) { boxMode = v === "inline" ? "inline" : "native"; }
    // The application menu bar of the window: `Window.this.menu = document.$("menu#app")`. The shell shows
    // it as the system menu bar on macOS and Windows (elsewhere the page draws its own); null removes it.
    get menu() { return appMenu; }
    set menu(el) { appMenu = el || null; N.menuSet(el ? el._id : -1); }
    get isActive() { return this === thisWindow ? active : this._valid; }
    get isValid() { return this === thisWindow || this._valid; }
    // Asks the window to close; a `closerequest` listener may cancel it. `value` is what the opener's
    // `modal()` resolves with.
    close(value) { N.windowClose(this === thisWindow ? 0 : this._id, JSON.stringify(value === undefined ? null : value)); return true; }
    focus() { N.windowFocus(this === thisWindow ? 0 : this._id); }
    postMessage(data) { N.windowPost(this === thisWindow ? 0 : this._id, JSON.stringify(data) ?? "null"); }
    get parent() { return this === thisWindow ? (info().opener === undefined ? null : refWindow(info().opener)) : this._parent || null; }
    get parameters() { return this === thisWindow ? info().parameters : this._parameters; }
    get caption() { return this === thisWindow ? document.title : this._caption; }
    set caption(v) { if (this === thisWindow) document.title = String(v); }
    // Events: `closerequest` (preventDefault() keeps it open) and `activate` on `Window.this`;
    // `close` on a window you opened.
    on(type, fn) { ((this._listeners ||= {})[type] ||= []).push(fn); return this; }
    off(type, fn) { const l = this._listeners && this._listeners[type]; if (l) l.splice(l.indexOf(fn) >>> 0, 1); return this; }
    addEventListener(type, fn) { return this.on(type, fn); }
    removeEventListener(type, fn) { return this.off(type, fn); }
  }
  Object.assign(Window, {
    POPUP_WINDOW: 1, TOOL_WINDOW: 2, CHILD_WINDOW: 3, FRAME_WINDOW: 4, DIALOG_WINDOW: 5,
    WINDOW_SHOWN: 1, WINDOW_MINIMIZED: 2, WINDOW_MAXIMIZED: 3, WINDOW_HIDDEN: 4, WINDOW_FULL_SCREEN: 5,
    CLOSE_BY_CHROME: 0, CLOSE_BY_CODE: 1, CLOSE_BY_LOAD: 2,
  });
  const TYPES = ["", "popup", "tool", "child", "frame", "dialog"], STATES = ["", "shown", "minimized", "maximized", "hidden", "fullscreen"];
  // How this window was opened: its id, the window that opened it, and its `parameters`.
  let infoCache = null;
  const info = () => {
    if (!infoCache) {
      const [id, opener, , parameters] = String(N.windowInfo() || "").split("\t");
      let params;
      try { params = parameters ? JSON.parse(parameters) : undefined; } catch { params = undefined; }
      infoCache = { id: +id || 0, opener: opener ? +opener : undefined, parameters: params };
    }
    return infoCache;
  };
  const known = new Map();
  let active = true;
  // A handle for another window by id (messages come from it, `parent` returns it).
  const refWindow = (id) => {
    let w = known.get(id);
    if (!w) {
      w = Object.create(Window.prototype);
      Object.assign(w, { _id: id, _valid: true, _listeners: {} });
      w.closed = new Promise((resolve) => { w._resolve = resolve; });
      known.set(id, w);
    }
    return w;
  };
  function openWindow(w, p, modal) {
    const q = [];
    const put = (k, v) => { if (v !== undefined && v !== null) q.push(encodeURIComponent(k) + "=" + encodeURIComponent(String(v))); };
    for (const k of ["url", "html", "caption", "x", "y", "width", "height", "alignment", "screen", "client"]) put(k, p[k]);
    put("type", TYPES[p.type] || (typeof p.type === "string" ? p.type : "frame"));
    put("state", STATES[p.state] || (typeof p.state === "string" ? p.state : undefined));
    put("parent", p.parent && p.parent._id !== undefined ? p.parent._id : undefined);
    if (p.parameters !== undefined) put("parameters", JSON.stringify(p.parameters));
    const id = N.windowOpen(q.join("&"), !!modal);
    Object.assign(w, { _id: id, _valid: true, _listeners: {}, _parent: thisWindow, _parameters: p.parameters, _caption: p.caption });
    w.closed = new Promise((resolve) => { w._resolve = resolve; });
    known.set(id, w);
    return w;
  }
  const fireWindow = (w, type, props) => {
    const e = { type, defaultPrevented: false, ...props, preventDefault() { e.defaultPrevented = true; } };
    for (const fn of [...((w._listeners && w._listeners[type]) || [])]) fn.call(w, e);
    return e;
  };
  Object.defineProperties(g, {
    // From the shell: the user (or `close()`) wants this window closed.
    __closeRequest: { value: (reason) => !fireWindow(thisWindow, "closerequest", { reason }).defaultPrevented },
    __windowActivate: { value: (on) => { active = !!on; fireWindow(thisWindow, "activate", { reason: on ? 1 : 0 }); } },
    __windowMessage: {
      value(from, data) {
        const e = new Event("message");
        e.data = data; e.origin = ""; e.source = refWindow(from);
        g.dispatchEvent(e);
      },
    },
    // A window this one opened has closed.
    __windowClosed: {
      value(id, value) {
        const w = known.get(id);
        if (!w) return;
        w._valid = false;
        fireWindow(w, "close", { value });
        w._resolve(value);
      },
    },
  });
  // `Window.share`: values shared by every window of the process. They cross as JSON, so only plain
  // data (no functions or elements) can be stored.
  const shared = new Proxy({}, {
    get: (_, k) => { if (typeof k === "symbol") return undefined; const v = N.shareGet(String(k)); return v === undefined ? undefined : JSON.parse(v); },
    set: (_, k, v) => { N.shareSet(String(k), JSON.stringify(v) ?? "null"); return true; },
    deleteProperty: (_, k) => { N.shareDel(String(k)); return true; },
    has: (_, k) => N.shareGet(String(k)) !== undefined,
    ownKeys: () => String(N.shareKeys() || "").split("\n").filter(Boolean),
    getOwnPropertyDescriptor: (_, k) => (N.shareGet(String(k)) === undefined ? undefined : { value: JSON.parse(N.shareGet(String(k))), enumerable: true, configurable: true, writable: true }),
  });
  let boxMode = "native";
  let popupWindows = false;
  const BOX_ICONS = { info: "i", alert: "!", error: "×", question: "?" };
  // The in-window message box: `.nile-modal` (backdrop) > `.nile-messagebox.<kind>` with a title, a body
  // (icon + content) and the buttons, all plain elements so CSS can restyle them.
  function inlineBox(kind, attrs, kids, caption) {
    return new Promise((resolve) => {
      const list = Array.isArray(kids) ? kids : [kids];
      const isVnode = (v) => Array.isArray(v) && typeof v[0] === "string";
      const specs = list.filter((v) => isVnode(v) && v[0] === "button").map((v) => ({
        text: textOf(v[2]).trim(), value: (v[1] && (v[1].value ?? v[1].id)) ?? textOf(v[2]).trim(),
        isDefault: !!(v[1] && v[1].default !== undefined), isCancel: !!(v[1] && v[1].cancel !== undefined),
      }));
      const content = list.filter((v) => !(isVnode(v) && v[0] === "button"));
      if (!specs.length) {
        if (kind === "question") specs.push({ text: "No", value: "no", isCancel: true }, { text: "Yes", value: "yes", isDefault: true });
        else specs.push({ text: "OK", value: "yes", isDefault: true, isCancel: true });
      }
      const backdrop = document.createElement("div");
      backdrop.className = "nile-modal";
      const box = document.createElement("div");
      box.className = "nile-messagebox " + kind;
      box.setAttribute("role", kind === "question" ? "dialog" : "alertdialog");
      if (caption) { const t = document.createElement("div"); t.className = "nile-messagebox-title"; t.textContent = caption; box.appendChild(t); }
      const body = document.createElement("div");
      body.className = "nile-messagebox-body";
      const icon = document.createElement("div");
      icon.className = "nile-messagebox-icon";
      icon.textContent = BOX_ICONS[kind];
      const text = document.createElement("div");
      text.className = "nile-messagebox-text";
      text.content(content);
      body.append(icon, text);
      const row = document.createElement("div");
      row.className = "nile-messagebox-buttons";
      box.append(body, row);
      backdrop.appendChild(box);
      let done = false;
      const finish = (value) => {
        if (done) return;
        done = true;
        document.removeEventListener("keydown", onKey, true);
        backdrop.remove();
        resolve(value);
      };
      for (const spec of specs) {
        const b = document.createElement("button");
        b.textContent = spec.text;
        if (spec.isDefault) b.className = "default";
        b.addEventListener("click", () => finish(spec.value));
        row.appendChild(b);
        spec.el = b;
      }
      const cancel = specs.find((x) => x.isCancel);
      const fallback = specs.find((x) => x.isDefault) || specs[0];
      function onKey(e) {
        if (e.key === "Escape") { e.preventDefault(); finish(cancel ? cancel.value : null); }
        else if (e.key === "Enter" && !(e.target && e.target.localName === "button")) { e.preventDefault(); finish(fallback.value); }
      }
      // Presses behind the box go nowhere.
      backdrop.addEventListener("mousedown", (e) => { if (e.target === backdrop) e.preventDefault(); });
      document.body.appendChild(backdrop);
      document.addEventListener("keydown", onKey, true);
      (specs.find((x) => x.isDefault) || specs[0]).el.focus();
    });
  }
  const thisWindow = Object.create(Window.prototype);
  g.Window = Window;

  // ---- popups and airborne elements -----------------------------------------------
  // `el.popup(popup, params)` shows an element (or JSX) above the page next to `el`; `el.takeOff()`
  // lifts an element out of the flow. Sciter draws them in separate OS windows; Nile paints them in
  // this window as `position: fixed` boxes, so they can't leave the window. See
  // docs.sciter.com/docs/DOM/Element (popup, takeOff) and /docs/DOM/out-of-canvas-elements.
  const openPopups = [];
  const SAVED = ["position", "display", "left", "top", "width", "height", "z-index", "visibility", "margin", "opacity", "transform", "transform-origin", "transition"];
  // NUMPAD placement: 1 bottom-left ... 9 top-right. Offsets of that point inside a w x h box.
  const padPoint = (n, w, h) => {
    const col = (((n | 0) - 1) % 3 + 3) % 3, row = Math.floor(((n | 0) - 1) / 3);
    return [(w * col) / 2, row === 0 ? h : row === 1 ? h / 2 : 0];
  };
  const fire = (target, type, source) => {
    const e = new Event(type, { bubbles: true });
    if (source) e.source = source;
    target.dispatchEvent(e);
  };
  class PopupWindow {
    constructor(rec) { this._r = rec; }
    get root() { return this._r.el; }
    get anchor() { return this._r.anchor || null; }
    get isValid() { return this._r.open; }
    get isTopmost() { return this._r.topmost; }
    set isTopmost(v) { this._r.topmost = !!v; }
    get isResizable() { return !!this._r.resizable; }
    set isResizable(v) { this._r.resizable = !!v; }
    get isMousePassthrough() { return !!this._r.passthrough; }
    set isMousePassthrough(v) { this._r.passthrough = !!v; this._r.el.style.pointerEvents = v ? "none" : ""; }
    on(type, cb) { (this._r.listeners[type] ||= []).push(cb); return this; }
    close() { closePopup(this._r); }
  }
  const lift = (el) => {
    const rec = { el, open: true, listeners: {}, topmost: true, saved: {}, attached: el.parentNode };
    for (const k of SAVED) rec.saved[k] = el.style.getPropertyValue(k);
    rec.next = el.nextSibling;
    if (!el.parentNode || !document.body.contains(el)) document.body.appendChild(el);
    return rec;
  };
  // A popup in an OS window of its own: tell the shell where it is (viewport coordinates, which may
  // lie outside the window).
  const syncPortal = (rec) => {
    const r = rec.el.getBoundingClientRect();
    N.portalSet(rec.el._id, r.left, r.top, r.width, r.height);
  };
  const place = (rec, left, top) => {
    rec.el.style.left = Math.round(left) + "px";
    rec.el.style.top = Math.round(top) + "px";
    if (rec.portal) syncPortal(rec);
    for (const cb of rec.listeners.move || []) cb({ type: "move", x: left, y: top });
  };
  function closePopup(rec) {
    if (!rec.open) return;
    // Anything opened after this one goes with it.
    const at = openPopups.indexOf(rec);
    if (at >= 0) for (const later of openPopups.splice(at + 1)) closePopup(later);
    if (rec.popup) fire(rec.el, "popup-dismissing");
    for (const cb of rec.listeners.close || []) cb({ type: "close" });
    rec.open = false;
    if (rec.portal) N.portalClear(rec.el._id);
    const i = openPopups.indexOf(rec);
    if (i >= 0) openPopups.splice(i, 1);
    for (const k of SAVED) {
      if (rec.saved[k]) rec.el.style.setProperty(k, rec.saved[k]);
      else rec.el.style.removeProperty(k);
    }
    if (rec.created) rec.el.remove();
    if (rec.popup && rec.anchor) fire(rec.anchor, "popup-dismissed");
    if (!openPopups.length) { document.removeEventListener("mousedown", outside, true); document.removeEventListener("keydown", escape, true); }
  }
  // A press on a popup's own anchor closes it, and the click that follows must not open it again.
  let suppressed = null;
  function outside(e) {
    const t = e.target;
    suppressed = null;
    const inside = (rec) => rec.el.contains(t);
    // A press outside the newest popups closes them; a press inside one keeps it and those below.
    for (let i = openPopups.length - 1; i >= 0; i--) {
      if (!openPopups[i].popup) continue;
      if (inside(openPopups[i])) return;
      const anchor = openPopups[i].anchor;
      if (anchor && anchor.contains(t)) suppressed = anchor;
      closePopup(openPopups[i]);
    }
  }
  function escape(e) {
    if (e.key !== "Escape") return;
    const top = [...openPopups].reverse().find((r) => r.popup);
    if (top) closePopup(top);
  }
  const intoElement = (content) => {
    if (content instanceof Element) return [content, false];
    // One element (the usual `<popup>` or `<menu>` of JSX) is the popup itself; several get a wrapper.
    const scratch = document.createElement("div");
    scratch.content(content);
    const kids = Array.from(scratch.children);
    if (kids.length === 1 && scratch.childNodes.length === 1) { kids[0].remove(); return [kids[0], true]; }
    const holder = document.createElement("popup");
    while (scratch.firstChild) holder.appendChild(scratch.firstChild);
    return [holder, true];
  };
  const POINTS = ["bottom-left", "bottom-center", "bottom-right", "middle-left", "middle-center", "middle-right", "top-left", "top-center", "top-right"];
  // `popup-position` and `popup-animation(type: slide, axis: vertical, heading: start-to-end, duration: 100ms)`.
  const cssPopup = (el) => {
    const cs = getComputedStyle(el);
    const out = {};
    const pos = cs.getPropertyValue("popup-position");
    if (pos) { const [a, b] = pos.split(/\s+/).map((n) => POINTS.indexOf(n) + 1); if (a && b) out.position = [a, b]; }
    const anim = cs.getPropertyValue("popup-animation");
    if (anim) {
      out.animation = {};
      for (const m of anim.matchAll(/([a-z]+)\s*:\s*([\w.-]+)/g)) out.animation[m[1]] = m[2];
    }
    return out;
  };
  const animationParams = (p) => {
    const a = {};
    if (p.animationType) a.type = p.animationType;
    if (p.animationAxis) a.axis = p.animationAxis;
    if (p.animationHeading) a.heading = p.animationHeading;
    if (p.animationDuration !== undefined) a.duration = typeof p.animationDuration === "number" ? p.animationDuration + "ms" : String(p.animationDuration);
    return a;
  };
  const millis = (t) => { const m = /^([\d.]+)(ms|s)?$/.exec(String(t || "")); return m ? parseFloat(m[1]) * (m[2] === "s" ? 1000 : 1) : 150; };
  // Appearance animation: blend (fade), inflate (grow from the popup's reference point), slide, roll.
  function animateIn(rec, a, popupAt) {
    if (!a || !a.type) return;
    const st = rec.el.style, ms = millis(a.duration);
    const vertical = a.axis !== "horizontal", forward = a.heading !== "end-to-start";
    const col = (((popupAt | 0) - 1) % 3 + 3) % 3, row = Math.floor(((popupAt | 0) - 1) / 3);
    let from = {}, to = {};
    if (a.type === "blend") { from = { opacity: "0" }; to = { opacity: "1" }; }
    else if (a.type === "inflate") {
      st.setProperty("transform-origin", `${col * 50}% ${row === 0 ? 100 : row === 1 ? 50 : 0}%`);
      from = { transform: "scale(0.2)", opacity: "0" }; to = { transform: "scale(1)", opacity: "1" };
    } else if (a.type === "slide") {
      const d = (forward ? -1 : 1) * 24;
      from = { transform: vertical ? `translateY(${d}px)` : `translateX(${d}px)`, opacity: "0" }; to = { transform: "translate(0px, 0px)", opacity: "1" };
    } else if (a.type === "roll") {
      st.setProperty("transform-origin", vertical ? (forward ? "50% 0%" : "50% 100%") : (forward ? "0% 50%" : "100% 50%"));
      from = { transform: vertical ? "scaleY(0.01)" : "scaleX(0.01)" }; to = { transform: "scale(1)" };
    } else return;
    st.setProperty("transition", "none");
    for (const k in from) st.setProperty(k, from[k]);
    rec.el.getBoundingClientRect();
    st.setProperty("transition", `opacity ${ms}ms, transform ${ms}ms`);
    for (const k in to) st.setProperty(k, to[k]);
    setTimeout(() => {
      if (!rec.open) return;
      for (const k of ["transition", "transform", "transform-origin", "opacity"]) {
        if (rec.saved[k]) st.setProperty(k, rec.saved[k]); else st.removeProperty(k);
      }
    }, ms + 30);
  }
  Element.prototype.popup = function (content, params = {}) {
    const anchor = this;
    if (suppressed === anchor) { suppressed = null; return undefined; }
    const [el, created] = intoElement(content);
    fire(anchor, "popup-request", el);
    const rec = lift(el);
    Object.assign(rec, { anchor, popup: true, created });
    const st = el.style;
    st.setProperty("position", "fixed");
    st.setProperty("display", "block");
    st.setProperty("margin", "0");
    st.setProperty("left", "0px");
    st.setProperty("top", "0px");
    st.setProperty("z-index", "2147483000");
    st.setProperty("visibility", "hidden");
    const pr = el.getBoundingClientRect();
    const vw = window.innerWidth, vh = window.innerHeight;
    // Placement: the call's parameters, else the popup's (then the anchor's) `popup-position` CSS.
    const css = cssPopup(el) || cssPopup(anchor);
    if (params.popupAt === undefined && css.position) params = { ...params, popupAt: css.position[0], anchorAt: params.anchorAt ?? css.position[1] };
    let left, top;
    if (params.x !== undefined || params.y !== undefined) {
      const [ox, oy] = padPoint(params.popupAt || 7, pr.width, pr.height);
      left = (params.x || 0) - ox; top = (params.y || 0) - oy;
    } else {
      const ar = anchor.getBoundingClientRect();
      const [ax, ay] = padPoint(params.anchorAt || 1, ar.width, ar.height);
      const [px, py] = padPoint(params.popupAt || 7, pr.width, pr.height);
      left = ar.left + ax - px; top = ar.top + ay - py;
      // Below doesn't fit but above does: flip to the anchor's top edge.
      if (top + pr.height > vh && ar.top - pr.height >= 0 && (params.anchorAt || 1) <= 3) top = ar.top - pr.height;
    }
    rec.portal = params.window === "popup" || (params.window === undefined && popupWindows);
    if (!rec.portal) {
      left = Math.max(0, Math.min(left, vw - pr.width));
      top = Math.max(0, Math.min(top, vh - pr.height));
    }
    place(rec, left, top);
    st.removeProperty("visibility");
    animateIn(rec, { ...css.animation, ...animationParams(params) }, params.popupAt || 7);
    // The UA menu look highlights `.nile-hover` (a UA `:hover` rule would make every page restyle on hover).
    let lit = null;
    el.addEventListener("mouseover", (e) => {
      const item = e.target.closest && e.target.closest("li");
      if (item === lit) return;
      if (lit) lit.classList.remove("nile-hover");
      lit = item && !item.hasAttribute("disabled") ? item : null;
      if (lit) lit.classList.add("nile-hover");
    });
    el.addEventListener("mouseout", (e) => { if (lit && !el.contains(e.relatedTarget || null)) { lit.classList.remove("nile-hover"); lit = null; } });
    // A menu closes when one of its items is clicked.
    el.addEventListener("click", (e) => {
      const item = e.target.closest && e.target.closest("li, [role=menuitem]");
      if (item && el.querySelector("menu, li") && !item.hasAttribute("disabled") && !item.querySelector("menu")) setTimeout(() => closePopup(rec), 0);
    });
    if (!openPopups.length) { document.addEventListener("mousedown", outside, true); document.addEventListener("keydown", escape, true); }
    openPopups.push(rec);
    fire(el, "popup-ready", el);
    return new PopupWindow(rec);
  };
  const airborne = new WeakMap();
  Element.prototype.takeOff = function (params) {
    if (params === undefined) {
      const rec = airborne.get(this);
      if (rec) { airborne.delete(this); closePopup(rec); }
      return undefined;
    }
    let rec = airborne.get(this);
    if (!rec) {
      rec = lift(this);
      rec.airborne = true;
      airborne.set(this, rec);
      openPopups.push(rec);
      this.style.setProperty("position", "fixed");
      this.style.setProperty("margin", "0");
      this.style.setProperty("z-index", "2147483000");
    }
    const st = this.style;
    let [x, y] = [params.x || 0, params.y || 0];
    const rel = params.relativeTo || "window";
    if (rel === "screen") { x -= window.screenX; y -= window.screenY; }
    else if (rel === "document") y -= window.scrollY;
    else if (rel === "parent" || rel === "self") {
      const base = (rel === "parent" ? rec.attached || document.body : this).getBoundingClientRect();
      x += base.left; y += base.top;
    }
    if (params.width !== undefined) st.setProperty("width", params.width + "px");
    if (params.height !== undefined) st.setProperty("height", params.height + "px");
    if (params.window === "popup" || params.window === "detached") rec.portal = true;
    if (params.isTopmost !== undefined) rec.topmost = !!params.isTopmost;
    if (params.isResizable !== undefined) rec.resizable = !!params.isResizable;
    place(rec, x, y);
    const win = new PopupWindow(rec);
    if (params.isMousePassthrough !== undefined) win.isMousePassthrough = params.isMousePassthrough;
    return win;
  };
  // ---- tooltips -----------------------------------------------------------------
  // After the pointer rests on an element the engine calls `__tooltipShow`: scripts may answer the
  // bubbling `tooltiprequest` event with `evt.source = element`; else the nearest `tooltip` (HTML) or
  // `title` (text) attribute becomes a `popup[role=tooltip]` near the pointer.
  let tip = null;
  const hideTip = () => {
    if (!tip) return;
    const t = tip;
    tip = null;
    if (t.created) t.el.remove();
    else for (const k of SAVED) { if (t.saved[k]) t.el.style.setProperty(k, t.saved[k]); else t.el.style.removeProperty(k); }
  };
  Object.defineProperty(g, "__tooltipHide", { value: hideTip });
  Object.defineProperty(g, "__tooltipShow", {
    value(id, x, y) {
      hideTip();
      const target = wrap(id);
      const ask = new Event("tooltiprequest", { bubbles: true, cancelable: true });
      ask.source = null;
      target.dispatchEvent(ask);
      let el = ask.source instanceof Element ? ask.source : null, created = false;
      if (!el) {
        let host = target;
        while (host && host.nodeType === 1 && !host.hasAttribute("tooltip") && !host.hasAttribute("title")) host = host.parentElement;
        if (!host || host.nodeType !== 1) return false;
        el = document.createElement("popup");
        el.setAttribute("role", "tooltip");
        if (host.hasAttribute("tooltip")) el.innerHTML = host.getAttribute("tooltip"); else el.textContent = host.getAttribute("title");
        if (!el.textContent.trim() && !el.children.length) return false;
        created = true;
        document.body.appendChild(el);
      }
      const rec = { el, created, saved: {} };
      for (const k of SAVED) rec.saved[k] = el.style.getPropertyValue(k);
      const st = el.style;
      st.setProperty("position", "fixed"); st.setProperty("display", "block"); st.setProperty("margin", "0");
      st.setProperty("z-index", "2147483200"); st.setProperty("left", "0px"); st.setProperty("top", "0px"); st.setProperty("visibility", "hidden");
      const r = el.getBoundingClientRect();
      const left = Math.max(0, Math.min(x + 12, window.innerWidth - r.width));
      const top = y + 20 + r.height > window.innerHeight ? Math.max(0, y - 8 - r.height) : y + 20;
      st.setProperty("left", Math.round(left) + "px"); st.setProperty("top", Math.round(top) + "px"); st.removeProperty("visibility");
      tip = rec;
      return true;
    },
  });
  // A right click: scripts get `contextmenu` and may answer with `evt.source = menuElement`.
  Object.defineProperty(g, "__contextMenu", {
    value(id, x, y) {
      const target = wrap(id);
      const e = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 2 });
      target.dispatchEvent(e);
      if (e.source instanceof Element) target.popup(e.source, { x, y, popupAt: 7 });
      return !e.defaultPrevented;
    },
  });

  // ---- `prototype: Class url(file.js)` ------------------------------------------------------------
  // The engine hands over every element whose style names a prototype (`__applyPrototypes`, after each
  // restyle). The class comes from the file (a module export named like the class, else the default
  // export) or from the page's own scripts; each element becomes an instance of it: fields run, its
  // `"on click at a"` methods are subscribed, `componentDidMount()` is called.
  const protoClasses = new Map();
  const protoWaiting = new Map();
  const protoBound = new WeakMap();
  const protoClass = (name, ns) => {
    if (ns) {
      if (typeof ns[name] === "function") return ns[name];
      for (const v of Object.values(ns)) if (typeof v === "function" && v.name === name) return v;
      if (typeof ns.default === "function") return ns.default;
    }
    if (/^[A-Za-z_$][\w$]*$/.test(name)) {
      try { const c = (0, eval)(name); if (typeof c === "function") return c; } catch (_) { /* not defined (yet) */ }
    }
    return null;
  };
  // `"on ^mousedown at >strip>label:not(:current)"(event, matched)`: the events, then an optional selector.
  const handlerMethods = (Cls) => {
    const out = [];
    for (let p = Cls.prototype; p && p !== Element.prototype; p = Object.getPrototypeOf(p)) {
      for (const name of Object.getOwnPropertyNames(p)) {
        const m = /^on\s+(.*)$/.exec(name);
        if (!m || typeof p[name] !== "function") continue;
        const at = m[1].search(/\s+at\s+/);
        const events = (at < 0 ? m[1] : m[1].slice(0, at)).trim();
        const selector = at < 0 ? null : m[1].slice(at).replace(/^\s+at\s+/, "").trim();
        out.push({ name, events, selector });
      }
    }
    return out;
  };
  function protoBind(el, Cls) {
    const old = protoBound.get(el);
    if (old === Cls) return;
    if (old) {
      for (const h of el._protoSubs || []) el.off(h);
      if (el._comp) { el._comp = false; liveComps--; if (typeof el.componentWillUnmount === "function") { try { el.componentWillUnmount(); } catch (e) { report(e); } } }
    }
    // Run the constructor and field initialisers on a throwaway instance of the element, and keep
    // the fields it set; the element's own wrapper stays the one scripts already hold.
    adopting = el._id;
    let inst = null;
    try { inst = new Cls(); } catch (e) { report(e); } finally { adopting = -1; }
    cache.set(el._id, el);
    if (inst) for (const k of Object.keys(inst)) if (!Object.prototype.hasOwnProperty.call(el, k)) el[k] = inst[k];
    Object.setPrototypeOf(el, Cls.prototype);
    protoBound.set(el, Cls);
    el._comp = true;
    liveComps++;
    el._protoSubs = [];
    for (const { name, events, selector } of handlerMethods(Cls)) {
      const fn = Cls.prototype[name];
      const handler = function (ev, matched) {
        const r = fn.call(el, ev, matched);
        // Returning true means the event is dealt with: nothing else sees or does anything about it.
        if (r === true) { ev.stopPropagation(); ev.preventDefault(); }
        return r;
      };
      el._protoSubs.push(handler);
      el.on(events, selector, handler);
    }
    if (typeof el.componentDidMount === "function") { try { el.componentDidMount(); } catch (e) { report(e); } }
    mounted();
  }
  Object.defineProperty(g, "__applyPrototypes", {
    value(list) {
      for (const [id, name, url] of list) {
        const el = wrap(id);
        if (!el) continue;
        const key = url + "\n" + name;
        const known = protoClasses.get(key);
        if (known) { protoBind(el, known); continue; }
        if (known === null) continue; // the file or class failed to load
        if (!url) {
          const C = protoClass(name);
          if (C) { protoClasses.set(key, C); protoBind(el, C); }
          continue; // a class a later script defines is picked up on the next restyle
        }
        const waiting = protoWaiting.get(key);
        if (waiting) { waiting.add(id); continue; }
        protoWaiting.set(key, new Set([id]));
        const where = /^[a-z][\w+.-]*:\/\//i.test(url) || url.startsWith("/") ? url : __DIR__ + url;
        import(where).then((ns) => {
          const C = protoClass(name, ns);
          protoClasses.set(key, C);
          if (!C) console.error("prototype: " + name + " is not exported by " + url);
          for (const i of protoWaiting.get(key) || []) { const e = wrap(i); if (e && C) protoBind(e, C); }
          protoWaiting.delete(key);
        }, (err) => {
          protoClasses.set(key, null);
          protoWaiting.delete(key);
          console.error("prototype: cannot load " + url + ": " + (err && err.message ? err.message : err));
        });
      }
    },
  });

  // ---- customElements ------------------------------------------------------------------------------
  // The web's way to bind a class to a tag, on the same machinery as `prototype:` (`protoBind`): matching
  // elements, present or added later, become instances; `connectedCallback`, `disconnectedCallback` and
  // `attributeChangedCallback` follow a MutationObserver. `attachShadow()` has no encapsulation: it
  // returns the element itself.
  const ceDefs = new Map();
  const ceWaiting = new Map();
  const ceDefOf = (el) => {
    if (el.nodeType !== 1) return null;
    const tag = el.localName;
    const d = ceDefs.get(tag);
    if (d && !d.ext) return d;
    const is = el.getAttribute("is");
    const x = is && ceDefs.get(is);
    return x && x.ext === tag ? x : null;
  };
  const ceUpgrade = (el, d) => {
    if (el._ce) return;
    el._ce = 1;
    if (!(el instanceof d.Cls)) protoBind(el, d.Cls);
    if (d.attrs.length && typeof el.attributeChangedCallback === "function") {
      for (const n of d.attrs) {
        const v = el.getAttribute(n);
        if (v !== null) { try { el.attributeChangedCallback(n, null, v); } catch (e) { report(e); } }
      }
    }
  };
  const ceConnect = (el) => {
    const d = ceDefOf(el);
    if (!d) return;
    ceUpgrade(el, d);
    if (el.isConnected && !el._ceOn) {
      el._ceOn = 1;
      if (typeof el.connectedCallback === "function") { try { el.connectedCallback(); } catch (e) { report(e); } }
    }
  };
  const ceDisconnect = (el) => {
    if (el._ceOn && !el.isConnected) {
      el._ceOn = 0;
      if (typeof el.disconnectedCallback === "function") { try { el.disconnectedCallback(); } catch (e) { report(e); } }
    }
  };
  const ceAll = (n) => (n.nodeType === 1 ? [n, ...n.querySelectorAll("*")] : []);
  let ceWatch = null;
  const ceObserve = () => {
    if (!ceWatch) {
      ceWatch = new MutationObserver((records) => {
        for (const r of records) {
          for (const n of r.removedNodes) for (const e of ceAll(n)) ceDisconnect(e);
          for (const n of r.addedNodes) for (const e of ceAll(n)) ceConnect(e);
        }
      });
    }
    ceWatch.observe(document, { childList: true, subtree: true });
  };
  g.customElements = {
    define(name, Cls, options) {
      name = String(name);
      if (!/^[a-z][^\sA-Z]*-[^\sA-Z]*$/.test(name)) throw new SyntaxError(`"${name}" is not a valid custom element name`);
      if (typeof Cls !== "function") throw new TypeError("customElements.define needs a class");
      if (ceDefs.has(name)) throw new Error(`"${name}" is already defined`);
      const ext = options && options.extends ? String(options.extends).toLowerCase() : null;
      const attrs = Array.from(Cls.observedAttributes || [], String);
      const d = { Cls, name, ext, attrs };
      ceDefs.set(name, d);
      customTags.set(Cls, ext || name);
      ceObserve();
      for (const el of document.querySelectorAll(ext ? `${ext}[is="${name}"]` : name)) ceConnect(el);
      const w = ceWaiting.get(name);
      if (w) { ceWaiting.delete(name); w.forEach((f) => f(Cls)); }
    },
    get(name) { const d = ceDefs.get(String(name)); return d ? d.Cls : undefined; },
    getName(Cls) { for (const d of ceDefs.values()) if (d.Cls === Cls) return d.name; return null; },
    whenDefined(name) {
      name = String(name);
      const d = ceDefs.get(name);
      if (d) return Promise.resolve(d.Cls);
      return new Promise((res) => { if (!ceWaiting.has(name)) ceWaiting.set(name, []); ceWaiting.get(name).push(res); });
    },
    upgrade(root) { for (const e of ceAll(root)) ceConnect(e); },
  };
  {
    const D = Object.getPrototypeOf(document);
    const make = D.createElement;
    D.createElement = function (tag, options) {
      const el = make.call(this, tag);
      const name = options && options.is ? String(options.is) : String(tag).toLowerCase();
      const d = ceDefs.get(name);
      if (d && (d.ext ? d.ext === String(tag).toLowerCase() : true)) {
        if (d.ext) el.setAttribute("is", name);
        ceUpgrade(el, d);
      }
      return el;
    };
    // `attributeChangedCallback` is called from the setters themselves, so detached elements report too.
    for (const m of ["setAttribute", "removeAttribute"]) {
      const orig = Element.prototype[m];
      Element.prototype[m] = function (n, v) {
        if (!this._ce) return orig.call(this, n, v);
        n = String(n);
        const old = this.getAttribute(n);
        const r = orig.call(this, n, v);
        const now = this.getAttribute(n);
        const d = ceDefOf(this);
        if (d && old !== now && d.attrs.includes(n) && typeof this.attributeChangedCallback === "function") {
          try { this.attributeChangedCallback(n, old, now); } catch (e) { report(e); }
        }
        return r;
      };
    }
    Object.defineProperties(Element.prototype, {
      shadowRoot: { get() { return this._shadow ? this : null; }, configurable: true },
      attachShadow: { value() { this._shadow = true; return this; }, configurable: true, writable: true },
    });
  }

  // ---- date, time and colour pickers ------------------------------------------------------------
  // The engine calls `__pickerOpen(id)` when the glyph of a date/time/colour input is clicked (or Alt+Down
  // is pressed in one). The picker is an ordinary element shown with `popup()`.
  const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const DOW = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
  const z2 = (n) => String(n).padStart(2, "0");
  const daysIn = (y, m) => new Date(Date.UTC(y, m, 0)).getUTCDate();
  const dateOf = (v) => { const m = /^(\d{4,})-(\d\d)-(\d\d)/.exec(v || ""); return m ? [+m[1], +m[2], +m[3]] : null; };
  const timeOf = (v) => { const m = /(\d\d):(\d\d)/.exec(String(v || "").split("T").pop()); return m ? [+m[1], +m[2]] : null; };
  const monthOf = (v) => { const m = /^(\d{4,})-(\d\d)$/.exec(v || ""); return m ? [+m[1], +m[2]] : null; };
  const isoWeek = (y, m, d) => {
    const t = new Date(Date.UTC(y, m - 1, d));
    t.setUTCDate(t.getUTCDate() - ((t.getUTCDay() + 6) % 7) + 3);
    const jan4 = new Date(Date.UTC(t.getUTCFullYear(), 0, 4));
    return [t.getUTCFullYear(), 1 + Math.round(((t - jan4) / 864e5 - 3 + ((jan4.getUTCDay() + 6) % 7)) / 7)];
  };
  const weekOf = (v) => {
    const m = /^(\d{4})-W(\d\d)$/.exec(v || "");
    if (!m) return null;
    const jan4 = new Date(Date.UTC(+m[1], 0, 4));
    const monday = new Date(jan4.getTime() - ((jan4.getUTCDay() + 6) % 7) * 864e5 + (+m[2] - 1) * 7 * 864e5);
    return [monday.getUTCFullYear(), monday.getUTCMonth() + 1, monday.getUTCDate()];
  };
  const todayParts = () => { const d = new Date(); return [d.getFullYear(), d.getMonth() + 1, d.getDate()]; };
  const ymd = (y, m, d) => `${String(y).padStart(4, "0")}-${z2(m)}-${z2(d)}`;
  const h = (tag, cls, text, attrs) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    if (attrs) for (const k of Object.keys(attrs)) e.setAttribute(k, attrs[k]);
    return e;
  };
  const setInput = (input, value, finished = true) => {
    if (input.value !== value) { input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); }
    if (finished) input.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const hsv2rgb = (hh, s, v) => {
    const f = (n) => { const k = (n + hh / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); };
    return [f(5), f(3), f(1)].map((x) => Math.round(x * 255));
  };
  const rgb2hsv = (r, g, b) => {
    r /= 255; g /= 255; b /= 255;
    const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn;
    let hh = 0;
    if (d) hh = mx === r ? ((g - b) / d + 6) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return [hh * 60, mx ? d / mx : 0, mx];
  };
  const hex = (rgb) => "#" + rgb.map((x) => z2(x.toString(16))).join("");
  const unhex = (v) => { const m = /^#?([0-9a-f]{6})$/i.exec(v || ""); return m ? [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16)) : null; };

  // A month grid; `pick(y, m, d)` is called with the chosen day.
  function calendar(input, kind, shown, selected, pick, week) {
    const box = h("div", "np-cal");
    const lo = input.getAttribute("min") || "", hiLimit = input.getAttribute("max") || "";
    const draw = () => {
      box.textContent = "";
      const head = h("div", "np-head");
      const prev = h("button", "np-nav", "‹", { type: "button", "aria-label": "Previous month" });
      const next = h("button", "np-nav", "›", { type: "button", "aria-label": "Next month" });
      head.append(prev, h("div", "np-title", MONTHS[shown[1] - 1] + " " + shown[0]), next);
      const move = (d) => { const t = new Date(Date.UTC(shown[0], shown[1] - 1 + d, 1)); shown = [t.getUTCFullYear(), t.getUTCMonth() + 1]; draw(); };
      prev.addEventListener("click", () => move(-1));
      next.addEventListener("click", () => move(1));
      box.append(head);
      const grid = h("div", "np-grid");
      for (const d of DOW) grid.append(h("span", "np-dow", d));
      const first = new Date(Date.UTC(shown[0], shown[1] - 1, 1)).getUTCDay();
      const today = todayParts();
      for (let i = 0; i < 42; i++) {
        const t = new Date(Date.UTC(shown[0], shown[1] - 1, 1 - first + i));
        const y = t.getUTCFullYear(), m = t.getUTCMonth() + 1, d = t.getUTCDate();
        const cell = h("button", "np-day", String(d), { type: "button" });
        if (m !== shown[1]) cell.classList.add("np-out");
        if (selected && selected[0] === y && selected[1] === m && selected[2] === d) cell.classList.add("np-sel");
        if (week && selected) {
          const a = isoWeek(y, m, d), b = isoWeek(selected[0], selected[1], selected[2]);
          if (a[0] === b[0] && a[1] === b[1]) cell.classList.add("np-sel");
        }
        if (today[0] === y && today[1] === m && today[2] === d) cell.classList.add("np-today");
        const iso = ymd(y, m, d);
        if ((lo && iso < lo.slice(0, 10)) || (hiLimit && iso > hiLimit.slice(0, 10))) cell.setAttribute("disabled", "");
        else cell.addEventListener("click", () => pick(y, m, d));
        grid.append(cell);
      }
      box.append(grid);
    };
    draw();
    return box;
  }
  // Show the chosen hour and minute about a third of the way down their columns.
  const revealChosen = (root) => {
    for (const l of root.querySelectorAll(".np-col")) {
      const at = Array.from(l.children).findIndex((c) => c.classList.contains("np-sel"));
      if (at > 0) l.scrollTop = Math.max(0, (at - 3) * 26);
    }
  };
  // Two scrolling columns of hours and minutes.
  function clock(input, current, pick) {
    const step = Math.max(1, Math.round((+input.getAttribute("step") || 60) / 60));
    const box = h("div", "np-clock");
    const col = (n, step_, value, set) => {
      const list = h("div", "np-col");
      for (let i = 0; i < n; i += step_) {
        const b = h("button", "np-cell" + (i === value ? " np-sel" : ""), z2(i), { type: "button" });
        b.addEventListener("click", () => set(i));
        list.append(b);
      }
      return list;
    };
    let [hh, mm] = current || [0, 0];
    const redraw = () => {
      box.textContent = "";
      const hours = col(24, 1, hh, (v) => { hh = v; pick(hh, mm, false); redraw(); });
      const mins = col(60, step, mm, (v) => { mm = v; pick(hh, mm, true); redraw(); });
      box.append(hours, mins);
      if (box.isConnected) revealChosen(box);
    };
    redraw();
    return box;
  }
  function monthGrid(input, selected, pick) {
    const box = h("div", "np-cal");
    let year = selected ? selected[0] : todayParts()[0];
    const draw = () => {
      box.textContent = "";
      const head = h("div", "np-head");
      const prev = h("button", "np-nav", "‹", { type: "button" }), next = h("button", "np-nav", "›", { type: "button" });
      prev.addEventListener("click", () => { year--; draw(); });
      next.addEventListener("click", () => { year++; draw(); });
      head.append(prev, h("div", "np-title", String(year)), next);
      box.append(head);
      const grid = h("div", "np-months");
      MONTHS.forEach((name, i) => {
        const b = h("button", "np-cell" + (selected && selected[0] === year && selected[1] === i + 1 ? " np-sel" : ""), name.slice(0, 3), { type: "button" });
        b.addEventListener("click", () => pick(year, i + 1));
        grid.append(b);
      });
      box.append(grid);
    };
    draw();
    return box;
  }
  function colorPicker(input) {
    const start = unhex(input.value) || [0, 0, 0];
    let [hue, sat, val] = rgb2hsv(...start);
    const box = h("div", "np-color");
    const sv = h("div", "np-sv"), white = h("div", "np-white"), black = h("div", "np-black"), knob = h("div", "np-knob");
    sv.append(white, black, knob);
    const hueBar = h("div", "np-hue"), hueKnob = h("div", "np-hueknob");
    hueBar.append(hueKnob);
    const row = h("div", "np-row"), preview = h("div", "np-preview");
    const field = h("input", "np-hex", null, { type: "text", maxlength: "7", spellcheck: "false" });
    row.append(preview, field);
    const swatches = h("div", "np-swatches");
    const show = (commit) => {
      const rgb = hsv2rgb(hue, sat, val), value = hex(rgb);
      sv.style.backgroundColor = hex(hsv2rgb(hue, 1, 1));
      knob.style.left = Math.round(sat * 100) + "%";
      knob.style.top = Math.round((1 - val) * 100) + "%";
      hueKnob.style.left = Math.round((hue / 360) * 100) + "%";
      preview.style.backgroundColor = value;
      if (field.value !== value) field.value = value;
      if (commit !== undefined) setInput(input, value, commit);
    };
    const drag = (el, move) => {
      el.addEventListener("mousedown", (e) => {
        const r = el.getBoundingClientRect();
        const at = (ev) => move(Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)), Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height)));
        at(e); show(false);
        const onMove = (ev) => { at(ev); show(false); };
        const onUp = () => { document.removeEventListener("mousemove", onMove, true); document.removeEventListener("mouseup", onUp, true); show(true); };
        document.addEventListener("mousemove", onMove, true);
        document.addEventListener("mouseup", onUp, true);
      });
    };
    drag(sv, (x, y) => { sat = x; val = 1 - y; });
    drag(hueBar, (x) => { hue = x * 360; });
    field.addEventListener("input", () => { const c = unhex(field.value); if (c) { [hue, sat, val] = rgb2hsv(...c); show(false); } });
    field.addEventListener("change", () => { const c = unhex(field.value); if (c) { [hue, sat, val] = rgb2hsv(...c); show(true); } });
    for (const c of ["#000000", "#ffffff", "#e53935", "#fb8c00", "#fdd835", "#43a047", "#00acc1", "#1e88e5", "#5e35b1", "#d81b60", "#795548", "#757575"]) {
      const b = h("button", "np-swatch", null, { type: "button", "aria-label": c, title: c });
      b.style.backgroundColor = c;
      b.addEventListener("click", () => { [hue, sat, val] = rgb2hsv(...unhex(c)); show(true); });
      swatches.append(b);
    }
    box.append(sv, hueBar, row, swatches);
    show();
    return box;
  }
  Object.defineProperty(g, "__pickerOpen", {
    value(id) {
      const input = wrap(id);
      if (!input || input.localName !== "input") return;
      const kind = (input.getAttribute("type") || "").toLowerCase();
      const pop = h("div", "nile-picker", null, { role: "dialog", "aria-label": "Choose " + kind });
      let win;
      const close = () => { if (win) win.close(); };
      const foot = (extra) => {
        const f = h("div", "np-foot");
        const now = h("button", "np-link", kind === "time" ? "Now" : "Today", { type: "button" });
        const clear = h("button", "np-link", "Clear", { type: "button" });
        now.addEventListener("click", extra);
        clear.addEventListener("click", () => { setInput(input, ""); close(); });
        f.append(now, clear);
        return f;
      };
      if (kind === "date") {
        const sel = dateOf(input.value), t = sel || todayParts();
        pop.append(calendar(input, kind, [t[0], t[1]], sel, (y, m, d) => { setInput(input, ymd(y, m, d)); close(); }),
          foot(() => { const n = todayParts(); setInput(input, ymd(...n)); close(); }));
      } else if (kind === "week") {
        const sel = weekOf(input.value), t = sel || todayParts();
        pop.append(calendar(input, kind, [t[0], t[1]], sel, (y, m, d) => { const w = isoWeek(y, m, d); setInput(input, `${w[0]}-W${z2(w[1])}`); close(); }, true),
          foot(() => { const n = todayParts(), w = isoWeek(...n); setInput(input, `${w[0]}-W${z2(w[1])}`); close(); }));
      } else if (kind === "month") {
        const sel = monthOf(input.value);
        pop.append(monthGrid(input, sel, (y, m) => { setInput(input, `${String(y).padStart(4, "0")}-${z2(m)}`); close(); }),
          foot(() => { const n = todayParts(); setInput(input, `${String(n[0]).padStart(4, "0")}-${z2(n[1])}`); close(); }));
      } else if (kind === "time") {
        pop.append(clock(input, timeOf(input.value), (hh, mm, done) => { setInput(input, `${z2(hh)}:${z2(mm)}`, done); if (done) close(); }),
          foot(() => { const d = new Date(); setInput(input, `${z2(d.getHours())}:${z2(d.getMinutes())}`); close(); }));
      } else if (kind === "datetime-local") {
        const v = input.value, sel = dateOf(v), time = timeOf(v) || [0, 0], t = sel || todayParts();
        let day = sel, hm = time;
        const put = (finished) => { if (day) setInput(input, `${ymd(...day)}T${z2(hm[0])}:${z2(hm[1])}`, finished); };
        const row = h("div", "np-split");
        row.append(calendar(input, kind, [t[0], t[1]], sel, (y, m, d) => { day = [y, m, d]; put(true); close(); }),
          clock(input, time, (hh, mm, done) => { hm = [hh, mm]; if (!day) day = todayParts(); put(done); }));
        pop.append(row, foot(() => { const d = new Date(); setInput(input, `${ymd(d.getFullYear(), d.getMonth() + 1, d.getDate())}T${z2(d.getHours())}:${z2(d.getMinutes())}`); close(); }));
      } else if (kind === "color") {
        pop.append(colorPicker(input));
      } else return;
      // Presses on the picker keep the input's focus (except in its hex field).
      pop.addEventListener("mousedown", (e) => { if (!(e.target && e.target.localName === "input")) e.preventDefault(); });
      win = input.popup(pop, { anchorAt: 1, popupAt: 7 });
      if (!win) return;
      win.on("close", () => pop.remove());
      revealChosen(pop);
    },
  });

  // Files dropped on the window (not on a file input): a `drop` event with `dataTransfer.files`.
  Object.defineProperty(g, "__fileDrop", {
    value(id, paths) {
      const target = wrap(id);
      if (!target) return;
      const files = paths.map((p) => new File(p));
      const e = new Event("drop", { bubbles: true, cancelable: true });
      e.dataTransfer = { files, types: ["Files"], items: files.map((f) => ({ kind: "file", type: f.type, getAsFile: () => f })), dropEffect: "copy" };
      e.files = files;
      target.dispatchEvent(e);
    },
  });

  // ---- constraint validation ------------------------------------------------------------------
  // `validity`, `checkValidity()`, `setCustomValidity()` ... computed from the attributes, and the
  // `:valid` / `:invalid` flags kept up to date on input/change events, `value` assignments and
  // attribute changes (the flags are what the selectors match).
  const NOT_VALIDATED = new Set(["hidden", "button", "submit", "reset", "image"]);
  const kindOf = (el) => (el.getAttribute("type") || "text").toLowerCase();
  const validatable = (el) => {
    const t = el.localName;
    if (t === "input") { if (NOT_VALIDATED.has(kindOf(el))) return false; }
    else if (t !== "textarea" && t !== "select") return false;
    return !el.disabled && !el.hasAttribute("readonly") && !el.closest("datalist");
  };
  const numeric = (v) => (v === null || v === undefined || v === "" ? NaN : Number(v));
  const VALIDITY_KEYS = ["valueMissing", "typeMismatch", "patternMismatch", "tooLong", "tooShort", "rangeUnderflow", "rangeOverflow", "stepMismatch", "badInput", "customError"];
  function computeValidity(el) {
    const v = Object.fromEntries(VALIDITY_KEYS.map((k) => [k, false]));
    if (!validatable(el)) { v.valid = true; return v; }
    const tag = el.localName, kind = tag === "input" ? kindOf(el) : tag;
    const value = tag === "select" ? el.value : String(el.value ?? "");
    const required = el.hasAttribute("required");
    if (required) {
      if (kind === "checkbox") v.valueMissing = !el.checked;
      else if (kind === "radio") {
        const name = el.getAttribute("name");
        const scope = el.closest("form") || document;
        v.valueMissing = !name ? !el.checked : !scope.querySelectorAll("input[type=radio]").some((r) => r.getAttribute("name") === name && r.checked);
      } else v.valueMissing = value === "";
    }
    if (value !== "" && kind !== "checkbox" && kind !== "radio" && kind !== "select") {
      if (kind === "email" && !/^[^\s@]+@[^\s@]+$/.test(value)) v.typeMismatch = true;
      if (kind === "url") { try { new URL(value); } catch (_) { v.typeMismatch = true; } }
      const pat = el.getAttribute("pattern");
      if (pat && (kind === "text" || kind === "search" || kind === "tel" || kind === "url" || kind === "email" || kind === "password")) {
        try { v.patternMismatch = !new RegExp("^(?:" + pat + ")$", "u").test(value); } catch (_) { /* an invalid pattern is ignored */ }
      }
      const len = [...value].length, max = numeric(el.getAttribute("maxlength")), min = numeric(el.getAttribute("minlength"));
      if (!Number.isNaN(max) && len > max) v.tooLong = true;
      if (!Number.isNaN(min) && len < min) v.tooShort = true;
      const lo = el.getAttribute("min"), hi = el.getAttribute("max");
      if (kind === "number" || kind === "range") {
        const n = Number(value);
        if (Number.isNaN(n)) v.badInput = true;
        else {
          if (lo !== null && !Number.isNaN(numeric(lo)) && n < Number(lo)) v.rangeUnderflow = true;
          if (hi !== null && !Number.isNaN(numeric(hi)) && n > Number(hi)) v.rangeOverflow = true;
          const stepAttr = el.getAttribute("step");
          if (stepAttr !== "any") {
            const step = Number.isNaN(numeric(stepAttr)) || Number(stepAttr) <= 0 ? 1 : Number(stepAttr);
            const base = Number.isNaN(numeric(lo)) ? 0 : Number(lo);
            const k = (n - base) / step;
            if (Math.abs(k - Math.round(k)) > 1e-7) v.stepMismatch = true;
          }
        }
      } else if (kind === "date" || kind === "month" || kind === "week" || kind === "time" || kind === "datetime-local") {
        // The formats sort as text.
        if (lo && value < lo) v.rangeUnderflow = true;
        if (hi && value > hi) v.rangeOverflow = true;
      }
    }
    v.customError = !!el._customValidity;
    v.valid = !VALIDITY_KEYS.some((k) => v[k]);
    return v;
  }
  const MESSAGES = { customError: null, valueMissing: "Please fill out this field.", typeMismatch: "Please enter a valid value.", patternMismatch: "Please match the requested format.", tooLong: "Please shorten this text.", tooShort: "Please lengthen this text.", rangeUnderflow: "The value is too small.", rangeOverflow: "The value is too large.", stepMismatch: "Please enter a valid value.", badInput: "Please enter a number." };
  const defineOn = (name, desc) => Object.defineProperty(Element.prototype, name, { configurable: true, ...desc });
  const validity = (el) => (validatable(el) || el.localName === "input" || el.localName === "textarea" || el.localName === "select" ? computeValidity(el) : undefined);
  defineOn("validity", { get() { return validity(this); } });
  defineOn("willValidate", { get() { return validatable(this); } });
  defineOn("validationMessage", {
    get() {
      const v = computeValidity(this);
      if (v.valid) return "";
      return v.customError ? this._customValidity : MESSAGES[VALIDITY_KEYS.find((k) => v[k])] || "";
    },
  });
  const revalidate = (el) => {
    if (el.nodeType !== 1 || !(el.localName === "input" || el.localName === "textarea" || el.localName === "select")) return;
    if (!validatable(el)) { N.setState(el._id, "valid", false); N.setState(el._id, "invalid", false); return; }
    const ok = computeValidity(el).valid;
    N.setState(el._id, "valid", ok);
    N.setState(el._id, "invalid", !ok);
  };
  defineOn("setCustomValidity", { value(msg) { this._customValidity = String(msg ?? ""); revalidate(this); }, writable: true });
  defineOn("checkValidity", {
    value() {
      if (this.localName === "form" || this.localName === "fieldset") {
        let ok = true;
        for (const c of this.querySelectorAll("input, select, textarea")) if (!c.checkValidity()) ok = false;
        return ok;
      }
      if (!validatable(this)) return true;
      const ok = computeValidity(this).valid;
      revalidate(this);
      if (!ok) this.dispatchEvent(new Event("invalid", { cancelable: true }));
      return ok;
    },
    writable: true,
  });
  defineOn("reportValidity", {
    value() {
      const ok = this.checkValidity();
      if (!ok && this.localName !== "form" && this.focus) this.focus();
      return ok;
    },
    writable: true,
  });
  // Keep the flags current. Nothing is observed on a page without controls.
  const refreshAll = (root) => { for (const c of root.querySelectorAll("input, select, textarea")) revalidate(c); };
  g.__refreshValidity = (el) => revalidate(el);
  document.addEventListener("DOMContentLoaded", () => {
    const body = document.documentElement;
    if (!body.querySelector("input, select, textarea")) { watchControls(); return; }
    refreshAll(body);
    watchControls();
  });
  function watchControls() {
    const again = (e) => { const t = e.target; if (t && t.nodeType === 1) { revalidate(t); if (t.localName === "input" && kindOf(t) === "radio") refreshAll(t.closest("form") || document.documentElement); } };
    document.addEventListener("input", again, true);
    document.addEventListener("change", again, true);
    new MutationObserver((records) => {
      for (const r of records) {
        if (r.type === "attributes") revalidate(r.target);
        else for (const n of r.addedNodes) if (n.nodeType === 1) { revalidate(n); if (n.querySelectorAll) refreshAll(n); }
      }
    }).observe(document.documentElement, { attributes: true, childList: true, subtree: true, attributeFilter: ["required", "min", "max", "step", "pattern", "minlength", "maxlength", "type", "value", "disabled", "readonly", "checked"] });
  }
  Object.defineProperty(g, "__sciterModule", { value: sciterModule });
  Object.defineProperty(g, "__envModule", { value: envModule });
  // `@sys`: the file-system part of Sciter's NodeJS-style module (docs.sciter.com JS.runtime/module-sys),
  // over the `fsCall` native. Sync functions throw; the promise forms wrap them.
  const fsRun = (op, ...a) => {
    const r = N.fsCall(op, a);
    if (r.startsWith("E:")) { const e = new Error(r.slice(2)); e.code = r.slice(2, r.indexOf(":", 2)); throw e; }
    return r.slice(2);
  };
  const statOf = (s) => { const o = JSON.parse(s); o.st_ctime = o.st_mtime; return o; };
  const pathOfAny = (p) => String(p);
  const sync = {
    stat: (p) => { try { return statOf(fsRun("stat", pathOfAny(p))); } catch (e) { if (e.code === "ENOENT") return null; throw e; } },
    lstat: (p) => { try { return statOf(fsRun("stat", pathOfAny(p), "link")); } catch (e) { if (e.code === "ENOENT") return null; throw e; } },
    readdir: (p) => { try { return JSON.parse(fsRun("readdir", pathOfAny(p))); } catch (e) { if (e.code === "ENOENT" || e.code === "ENOTDIR") return null; throw e; } },
    readFile: (p) => bufferOf(b64ToBytes(fsRun("read", pathOfAny(p)))),
    writeFile: (p, data) => { fsRun("write", pathOfAny(p), bytesToB64(typeof data === "string" ? new TextEncoder().encode(data) : toBytes(data))); },
    unlink: (p) => { fsRun("unlink", pathOfAny(p)); },
    mkdir: (p) => { fsRun("mkdir", pathOfAny(p)); },
    rmdir: (p) => { fsRun("rmdir", pathOfAny(p)); },
    rename: (a, b) => { fsRun("rename", pathOfAny(a), pathOfAny(b)); },
    copyfile: (a, b) => { fsRun("copyfile", pathOfAny(a), pathOfAny(b)); },
    realpath: (p) => fsRun("realpath", pathOfAny(p)),
  };
  const sysFs = {
    sync,
    S_IFMT: 0o170000, S_IFDIR: 0o040000, S_IFCHR: 0o020000, S_IFBLK: 0o060000, S_IFREG: 0o100000, S_IFLNK: 0o120000, S_IFSOCK: 0o140000, S_IFIFO: 0o010000,
    UV_DIRENT_UNKNOWN: 0, UV_DIRENT_FILE: 1, UV_DIRENT_DIR: 2, UV_DIRENT_LINK: 3, UV_DIRENT_FIFO: 4, UV_DIRENT_SOCKET: 5, UV_DIRENT_CHAR: 6, UV_DIRENT_BLOCK: 7,
    splitpath(p) { const s = String(p).replace(/[\\/]+$/, ""); const i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\")); return i < 0 ? ["", s] : [s.slice(0, i), s.slice(i + 1)]; },
  };
  for (const [name, fn] of Object.entries(sync)) {
    // The promise form throws for a missing file (stat) where the sync form returns null.
    const strict = name === "stat" || name === "lstat" || name === "readdir";
    sysFs[name + "Sync"] = fn;
    sysFs[name] = (...a) => new Promise((resolve, reject) => {
      try { const v = fn(...a); if (strict && v === null) reject(Object.assign(new Error("ENOENT: no such file or directory: " + a[0]), { code: "ENOENT" })); else resolve(v); } catch (e) { reject(e); }
    });
  }
  sysFs.readfile = sysFs.readFile; sysFs.readfileSync = sysFs.readFileSync; sync.readfile = sync.readFile;
  // Files, sockets, processes and OS queries over the `sysCall`/`sysStart` natives (nile-script/src/sys.rs).
  // Blocking calls return promises that `__sysDone` settles.
  const sysRun = (op, ...a) => {
    const r = N.sysCall(op, a.map(String));
    if (r.startsWith("E:")) { const m = r.slice(2); const e = new Error(m); e.code = m.slice(0, m.indexOf(":")); throw e; }
    return r.slice(2);
  };
  const sysOps = new Map();
  let sysNext = 1;
  const sysAsync = (op, ...a) => new Promise((resolve, reject) => {
    const id = sysNext++;
    sysOps.set(id, { resolve, reject });
    N.sysStart(id, op, a.map(String));
  });
  Object.defineProperty(g, "__sysDone", {
    value(id, ok, text) {
      const p = sysOps.get(id);
      if (!p) return;
      sysOps.delete(id);
      if (ok) p.resolve(text); else p.reject(Object.assign(new Error(text), { code: text.slice(0, text.indexOf(":")) }));
    },
  });
  const later = (fn) => new Promise((resolve, reject) => { try { resolve(fn()); } catch (e) { reject(e); } });
  const readResult = (t) => (t === "E" ? undefined : b64ToBytes(t.slice(1)));
  const addrOf = (o) => [o && o.ip != null ? o.ip : "", o && o.port != null ? o.port : 0];
  class SysStream {
    constructor(id) { Object.defineProperty(this, "_id", { value: id, writable: true }); }
    read(len) { return sysAsync("read", this._id, len == null ? 65536 : len).then(readResult); }
    write(data) { return sysAsync("write", this._id, bytesToB64(toBytes(data))).then(Number); }
    close() { sysRun("close", this._id); }
    fileno() { return this._id; }
  }
  class TCP extends SysStream {
    constructor() { super(Number(sysRun("tcp_new"))); }
    connect(addr) { return sysAsync("connect", this._id, ...addrOf(addr)).then(() => undefined); }
    // Encrypt the connection (TLS, certificates checked against the bundled roots); `host` is the server's name.
    tls(host) { return sysAsync("tls", this._id, host).then(() => undefined); }
    bind(addr) { sysRun("bind", this._id, ...addrOf(addr)); }
    listen() { sysRun("listen", this._id); }
    accept() { return sysAsync("accept", this._id).then((r) => { const s = Object.create(TCP.prototype); Object.defineProperty(s, "_id", { value: Number(r), writable: true }); return s; }); }
    shutdown() { sysRun("shutdown", this._id); return Promise.resolve(); }
    getsockname() { return JSON.parse(sysRun("sockname", this._id)); }
    getpeername() { return JSON.parse(sysRun("peername", this._id)); }
  }
  // Named pipes: a Unix domain socket named by its path; where those don't exist the pipe is a
  // loopback socket announced in a file under the temp folder (so only Nile programs can reach it).
  class Pipe extends SysStream {
    constructor() { super(Number(sysRun("pipe_new"))); this._name = ""; }
    bind(name) { sysRun("pipe_bind", this._id, String(name)); this._name = String(name); }
    listen() { sysRun("listen", this._id); }
    connect(name) { return later(() => { sysRun("pipe_connect", this._id, String(name)); this._name = String(name); }); }
    accept() {
      return sysAsync("accept", this._id).then((r) => {
        const p = Object.create(Pipe.prototype);
        Object.defineProperty(p, "_id", { value: Number(r), writable: true });
        p._name = this._name;
        return p;
      });
    }
    getsockname() { return this._name; }
    getpeername() { return this._name; }
  }
  // Terminals: standard input (read) and output/error (write); `setMode` 0 normal, 1 raw, 2 raw input
  // with output processing; `getWinSize()` is the terminal's size.
  class TTY extends SysStream {
    constructor(fd) { super(Number(sysRun("tty_new", fd))); this._fd = fd; }
    setMode(mode) { sysRun("tty_mode", mode | 0); }
    getWinSize() { return JSON.parse(sysRun("tty_size")); }
  }
  // `fs.watch(path, (path, events) => ...)`: events is a bitmask, 1 for a rename (something created,
  // removed or renamed) and 2 for a change. Folders are watched with what is inside them. Written by
  // looking at the tree every 250 ms (no operating system notifications), so it stops at 20,000 entries.
  const WATCH_LIMIT = 20000;
  const snapshot = (root) => {
    const seen = new Map();
    const visit = (path) => {
      if (seen.size >= WATCH_LIMIT) return;
      const st = sync.stat(path);
      if (!st) return;
      seen.set(path, st.st_mtime + ":" + st.st_size + ":" + st.isDirectory);
      if (st.isDirectory) for (const entry of sync.readdir(path) || []) visit(path.replace(/[\\/]$/, "") + "/" + entry.name);
    };
    visit(root);
    return seen;
  };
  const watchFs = (path, callback) => {
    path = String(path);
    if (!sync.stat(path)) throw Object.assign(new Error("ENOENT: no such file or directory: " + path), { code: "ENOENT" });
    let before = snapshot(path);
    const timer = setInterval(() => {
      const now = snapshot(path);
      const events = [];
      for (const [p, sig] of now) { if (!before.has(p)) events.push([p, 1]); else if (before.get(p) !== sig) events.push([p, 2]); }
      for (const p of before.keys()) if (!now.has(p)) events.push([p, 1]);
      before = now;
      for (const [p, e] of events) { try { callback(p, e); } catch (err) { console.error(err && err.stack ? err.stack : String(err)); } }
    }, 250);
    return { path, close() { clearInterval(timer); } };
  };
  sysFs.watch = watchFs;
  class UDP {
    constructor() { Object.defineProperty(this, "_id", { value: Number(sysRun("udp_new")), writable: true }); }
    bind(addr) { sysRun("bind", this._id, ...addrOf(addr)); }
    connect(addr) { return sysAsync("connect", this._id, ...addrOf(addr)).then(() => undefined); }
    send(data, addr) { return sysAsync("send", this._id, bytesToB64(toBytes(data)), ...(addr ? addrOf(addr) : ["", 0])).then(Number); }
    recv(len) { return sysAsync("recv", this._id, len == null ? 65536 : len).then((t) => { const o = JSON.parse(t); o.data = b64ToBytes(o.data); return o; }); }
    close() { sysRun("close", this._id); }
    fileno() { return this._id; }
    getsockname() { return JSON.parse(sysRun("sockname", this._id)); }
    getpeername() { return JSON.parse(sysRun("peername", this._id)); }
  }
  class Process {
    constructor(info) {
      this.pid = info.pid;
      Object.defineProperty(this, "_id", { value: info.id });
      for (const k of ["stdin", "stdout", "stderr"]) this[k] = info[k] == null ? null : new SysStream(info[k]);
    }
    wait() { return sysAsync("wait", this._id).then(JSON.parse); }
    kill() { sysRun("kill", this._id); }
  }
  // `spawn(["ls", "-l"], { cwd, env, stdin, stdout, stderr })`: each stdio is "pipe" (default), "inherit" or "ignore".
  // `env` adds to the program's environment.
  const spawn = (args, opts = {}) => {
    const list = typeof args === "string" ? args.trim().split(/\s+/) : Array.from(args, String);
    const env = Object.entries(opts.env || {}).flat();
    return new Process(JSON.parse(sysRun("spawn", opts.cwd || "", opts.stdin || "pipe", opts.stdout || "pipe", opts.stderr || "pipe", env.length / 2, ...env, ...list)));
  };
  class SysFile {
    constructor(id, path) { Object.defineProperty(this, "_id", { value: id }); this.path = path; }
    readSync(len, pos) { return b64ToBytes(sysRun("fread", this._id, len == null ? "" : len, pos == null ? "" : pos)); }
    writeSync(data, pos) { return Number(sysRun("fwrite", this._id, bytesToB64(toBytes(data)), pos == null ? "" : pos)); }
    statSync() { return JSON.parse(sysRun("fstat", this._id)); }
    closeSync() { sysRun("close", this._id); }
    read(len, pos) { return later(() => this.readSync(len, pos)); }
    write(data, pos) { return later(() => this.writeSync(data, pos)); }
    stat() { return later(() => this.statSync()); }
    close() { return later(() => this.closeSync()); }
    fileno() { return this._id; }
  }
  const openSync = (path, flags = "r") => new SysFile(Number(sysRun("fopen", pathOfAny(path), flags)), String(path));
  sysFs.File = SysFile;
  sysFs.openSync = openSync;
  sysFs.open = (path, flags) => later(() => openSync(path, flags));
  sysFs.Dir = class Dir {
    constructor(path) { this.path = String(path); this._items = sync.readdir(path); if (this._items === null) throw Object.assign(new Error("ENOENT: no such directory: " + path), { code: "ENOENT" }); this._i = 0; }
    next() { return Promise.resolve(this._i < this._items.length ? this._items[this._i++] : undefined); }
    close() { this._i = this._items.length; }
    [Symbol.asyncIterator]() { return { next: () => this.next().then((v) => (v === undefined ? { done: true, value: undefined } : { done: false, value: v })) }; }
  };
  sysFs.opendir = (path) => later(() => new sysFs.Dir(path));
  sysFs.opendirSync = (path) => new sysFs.Dir(path);
  const sysModule = {
    fs: sysFs, TCP, UDP, Pipe, TTY, Process, spawn,
    hrtime: () => BigInt(Math.round(N.now() * 1e6)),
    gettimeofday: () => JSON.parse(sysRun("gettimeofday")),
    uname: () => JSON.parse(sysRun("uname")),
    isatty: (fd) => sysRun("isatty", fd) === "true",
    environ: () => JSON.parse(sysRun("environ")),
    getenv: (name) => { const v = sysRun("getenv", name); return v === "" ? undefined : v.slice(1); },
    setenv: (name, value) => { sysRun("setenv", name, value); },
    unsetenv: (name) => { sysRun("unsetenv", name); },
    cwd: () => sysRun("cwd"),
    chdir: (dir) => { sysRun("chdir", dir); },
    homedir: () => sysRun("homedir"),
    tmpdir: () => sysRun("tmpdir"),
    exepath: () => sysRun("exepath"),
    // Random bytes: a length gives a new `Uint8Array`; a typed array is filled in place. Not cryptographic.
    random: (target) => {
      const buf = typeof target === "number" ? new Uint8Array(target) : target;
      const bytes = b64ToBytes(sysRun("random", buf.byteLength));
      new Uint8Array(buf.buffer || buf, buf.byteOffset || 0, buf.byteLength).set(bytes);
      return buf;
    },
  };
  // ---- WebSocket (RFC 6455) over the TCP sockets above; `wss://` uses TLS when the build has it ------------
  const wsEvent = (type, props) => { const e = new Event(type); Object.assign(e, props); return e; };
  g.WebSocket = class WebSocket extends EventTarget {
    constructor(url, protocols) {
      super();
      const u = new URL(url);
      if (u.protocol !== "ws:" && u.protocol !== "wss:") throw new SyntaxError("the URL's scheme must be ws or wss");
      this.url = u.href;
      this.readyState = 0;
      this.protocol = "";
      this.extensions = "";
      this.bufferedAmount = 0;
      this.binaryType = "blob";
      this.onopen = this.onmessage = this.onerror = this.onclose = null;
      const wanted = protocols === undefined ? [] : Array.isArray(protocols) ? protocols : [protocols];
      this._tcp = null; this._buf = new Uint8Array(0); this._frag = null; this._closing = false; this._chain = Promise.resolve();
      this._run(u, wanted).catch((e) => this._fail(e));
    }
    async _run(u, wanted) {
      const secure = u.protocol === "wss:";
      const tcp = this._tcp = new sysModule.TCP();
      await tcp.connect({ ip: u.hostname, port: Number(u.port) || (secure ? 443 : 80) });
      if (secure) await tcp.tls(u.hostname);
      const key = bytesToB64(sysModule.random(16));
      const head = [`GET ${u.pathname || "/"}${u.search} HTTP/1.1`, `Host: ${u.host}`, "Upgrade: websocket", "Connection: Upgrade", `Sec-WebSocket-Key: ${key}`, "Sec-WebSocket-Version: 13"];
      if (wanted.length) head.push(`Sec-WebSocket-Protocol: ${wanted.join(", ")}`);
      await tcp.write(head.join("\r\n") + "\r\n\r\n");
      // The response head, then whatever frames came with it.
      let acc = new Uint8Array(0), end = -1;
      while (end < 0) {
        const chunk = await tcp.read();
        if (!chunk) throw new Error("the server closed the connection during the handshake");
        acc = this._cat(acc, chunk);
        for (let i = 0; i + 3 < acc.length; i++) if (acc[i] === 13 && acc[i + 1] === 10 && acc[i + 2] === 13 && acc[i + 3] === 10) { end = i; break; }
        if (acc.length > 65536) throw new Error("the handshake response is too long");
      }
      const text = new TextDecoder().decode(acc.subarray(0, end));
      if (!/^HTTP\/1\.1 101/.test(text)) throw new Error("the server did not switch protocols: " + text.split("\r\n")[0]);
      const sub = /^sec-websocket-protocol:\s*(.+)$/im.exec(text);
      if (sub) this.protocol = sub[1].trim();
      this._buf = acc.subarray(end + 4);
      if (this.readyState !== 0) return;
      this.readyState = 1;
      this._emit("open", {});
      this._frames();
      await this._loop();
    }
    _cat(a, b) { const o = new Uint8Array(a.length + b.length); o.set(a); o.set(b, a.length); return o; }
    _emit(type, props) {
      const e = wsEvent(type, props);
      try { this.dispatchEvent(e); } catch (err) { report(err); }
    }
    async _loop() {
      for (;;) {
        const chunk = await this._tcp.read();
        if (!chunk) break;
        this._buf = this._cat(this._buf, chunk);
        this._frames();
        if (this.readyState === 3) return;
      }
      this._finish(1006, "", false);
    }
    // Decode every complete frame in the buffer.
    _frames() {
      for (;;) {
        const b = this._buf;
        if (b.length < 2) return;
        const fin = (b[0] & 0x80) !== 0, op = b[0] & 15, masked = (b[1] & 0x80) !== 0;
        let len = b[1] & 127, at = 2;
        if (len === 126) { if (b.length < 4) return; len = (b[2] << 8) | b[3]; at = 4; }
        else if (len === 127) { if (b.length < 10) return; len = 0; for (let i = 2; i < 10; i++) len = len * 256 + b[i]; at = 10; }
        const mask = masked ? b.subarray(at, at + 4) : null;
        if (masked) at += 4;
        if (b.length < at + len) return;
        const payload = b.slice(at, at + len);
        if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i & 3];
        this._buf = b.subarray(at + len);
        this._frame(fin, op, payload);
        if (this.readyState === 3) return;
      }
    }
    _frame(fin, op, data) {
      if (op === 0 || op === 1 || op === 2) {
        if (op !== 0) this._frag = { op, parts: [] };
        if (!this._frag) return;
        this._frag.parts.push(data);
        if (!fin) return;
        const { op: kind, parts } = this._frag;
        this._frag = null;
        const all = partBytes(new Blob(parts));
        const payload = kind === 1 ? new TextDecoder().decode(all) : this.binaryType === "arraybuffer" ? all.buffer.slice(all.byteOffset, all.byteOffset + all.byteLength) : new Blob([all]);
        this._emit("message", { data: payload, origin: this.url, lastEventId: "", source: null });
      } else if (op === 8) {
        const code = data.length >= 2 ? (data[0] << 8) | data[1] : 1005;
        const reason = data.length > 2 ? new TextDecoder().decode(data.subarray(2)) : "";
        if (!this._closing) this._send(8, data.length >= 2 ? data.subarray(0, 2) : new Uint8Array(0));
        this._finish(code, reason, true);
      } else if (op === 9) this._send(10, data);
    }
    _send(op, payload) {
      const n = payload.length;
      const head = n < 126 ? [0x80 | op, 0x80 | n] : n < 65536 ? [0x80 | op, 0x80 | 126, n >> 8, n & 255] : [0x80 | op, 0x80 | 127, 0, 0, 0, 0, (n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
      const mask = sysModule.random(4);
      const body = new Uint8Array(n);
      for (let i = 0; i < n; i++) body[i] = payload[i] ^ mask[i & 3];
      const frame = this._cat(this._cat(Uint8Array.from(head), mask), body);
      this._chain = this._chain.then(() => this._tcp.write(frame)).catch(() => {});
      return this._chain;
    }
    send(data) {
      if (this.readyState === 0) throw new Error("InvalidStateError: the connection is still opening");
      if (this.readyState !== 1) return;
      if (typeof data === "string") this._send(1, new TextEncoder().encode(data));
      else this._send(2, partBytes(data));
    }
    close(code = 1000, reason = "") {
      if (this.readyState >= 2) return;
      if (this.readyState === 0) { this._fail(new Error("closed before the connection was established"), true); return; }
      this.readyState = 2;
      this._closing = true;
      const r = new TextEncoder().encode(String(reason));
      this._send(8, this._cat(Uint8Array.from([code >> 8, code & 255]), r));
      // A server that never answers doesn't keep the socket open for ever.
      setTimeout(() => this._finish(code, reason, false), 3000);
    }
    _finish(code, reason, clean) {
      if (this.readyState === 3) return;
      this.readyState = 3;
      try { this._tcp && this._tcp.close(); } catch {}
      this._emit("close", { code, reason, wasClean: clean });
    }
    _fail(err, quiet) {
      if (this.readyState === 3) return;
      if (!quiet) this._emit("error", { message: err && err.message });
      this._finish(1006, "", false);
    }
    static get CONNECTING() { return 0; } static get OPEN() { return 1; } static get CLOSING() { return 2; } static get CLOSED() { return 3; }
  };
  for (const [k, v] of [["CONNECTING", 0], ["OPEN", 1], ["CLOSING", 2], ["CLOSED", 3]]) Object.defineProperty(g.WebSocket.prototype, k, { value: v });
  Object.defineProperty(g, "__sysModule", { value: sysModule });

  // `@debug` (docs.sciter.com JS.runtime/module-debug): the handlers that make sense without an
  // inspector; the breakpoint/inspection functions exist but do nothing.
  const callFrame = (level) => {
    const lines = String(new Error().stack || "").split("\n").filter((l) => /^\s*at /.test(l));
    const l = lines[level + 1]; // 0 = this helper, 1 = callStackAt, so level 0 is its caller
    if (!l) return null;
    const m = /^\s*at (?:(.*?) \()?(.*?):(\d+)(?::\d+)?\)?$/.exec(l) || [];
    return { isNative: /native/.test(l), functionName: m[1] || "", functionLineNo: 0, fileName: m[2] || "", lineNo: +m[3] || 0 };
  };
  const debugModule = {
    setUnhandledExceptionHandler(f) { hooks.unhandled = typeof f === "function" ? f : null; },
    setConsoleOutputHandler(f) { hooks.console = typeof f === "function" ? f : null; },
    callStackAt(level) { return callFrame(level | 0); },
    setResourceArrivalHandler() {}, setBreakpointHandler() {}, setBreakpoints() {},
    getElementByUID(uid) { return wrap(uid); },
    getUIDofElement(el) { return el._id; },
    highlightElement() {}, getStyleRulesOfElement() { return []; }, containerId() { return 0; },
    objectKind(o) { return o === null ? "null" : Array.isArray(o) ? "array" : typeof o; },
    sublimatedValue(v) { return v; }, sublimatedValueElements() { return []; }, frameVariables() { return {}; },
  };
  Object.defineProperty(g, "__debugModule", { value: debugModule });

  // `@storage` (docs.sciter.com Storage): `open(path)` gives a store whose `root` object persists
  // on `commit()`/`close()`. The file is JSON (Dates and registered classes are tagged), not
  // Sciter's own format; indexes (`createIndex`) aren't implemented.
  const storageOpen = (path, allowWrite = true) => {
    path = String(path);
    const classes = new Map();
    let root = {};
    let text = null;
    try { text = new TextDecoder().decode(fsRead(path)); } catch (e) { if (e.code !== "ENOENT" && !allowWrite) return null; }
    const revive = (v) => {
      if (Array.isArray(v)) return v.map(revive);
      if (v && typeof v === "object") {
        if ("$date" in v) return new Date(v.$date);
        const o = {};
        for (const k of Object.keys(v)) if (k !== "$class") o[k] = revive(v[k]);
        if (v.$class && classes.has(v.$class)) Object.setPrototypeOf(o, classes.get(v.$class).prototype);
        return o;
      }
      return v;
    };
    const tag = (v, seen) => {
      if (v instanceof Date) return { $date: v.getTime() };
      if (Array.isArray(v)) return v.map((x) => tag(x, seen));
      if (v && typeof v === "object") {
        if (seen.has(v)) throw new Error("storage: circular reference");
        seen.add(v);
        const o = {};
        const cls = Object.getPrototypeOf(v) !== Object.prototype && Object.getPrototypeOf(v) !== null ? v.constructor && v.constructor.name : null;
        if (cls && classes.has(cls)) o.$class = cls;
        for (const k of Object.keys(v)) if (typeof v[k] !== "function") o[k] = tag(v[k], seen);
        seen.delete(v);
        return o;
      }
      return v;
    };
    let pending = text;
    const db = {
      get root() { if (pending !== null) { try { root = revive(JSON.parse(pending)); } catch (_) { root = {}; } pending = null; } return root; },
      set root(v) { pending = null; root = v; },
      registerClass(cls) { classes.set(cls.name, cls); },
      commit() {
        if (!allowWrite) return;
        fsWrite(path, JSON.stringify(tag(db.root, new Set())));
      },
      close() { db.commit(); },
      createIndex() { return null; },
    };
    return db;
  };
  const fsRead = (p) => b64ToBytes(fsRun("read", String(p)));
  const fsWrite = (p, text) => { fsRun("write", String(p), bytesToB64(new TextEncoder().encode(text))); };
  Object.defineProperty(g, "__storageModule", { value: { open: storageOpen } });
  }
