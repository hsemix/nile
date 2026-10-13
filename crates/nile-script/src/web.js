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
  }
  g.Request = class Request extends Body {
    constructor(input, init = {}) {
      super();
      const from = input instanceof Request ? input : null;
      this.url = from ? from.url : String(input);
      this.method = String(init.method || (from && from.method) || "GET").toUpperCase();
      this.headers = new Headers(init.headers || (from && from.headers));
      this.signal = init.signal || (from && from.signal) || null;
      this._init(init.body !== undefined ? init.body : from && from._bytes);
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
  g.File = class File {
    constructor(path) {
      this.path = path;
      this.name = path.split(/[\\/]/).pop();
      this.type = MIME[(this.name.split(".").pop() || "").toLowerCase()] || "";
      this.lastModified = 0;
    }
    get size() { return N.fileSize(this.path); }
    text() { return fetch(this.path).then((r) => r.text()); }
    arrayBuffer() { return fetch(this.path).then((r) => r.arrayBuffer()); }
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
  Object.defineProperty(g, "__sciterModule", { value: sciterModule });
  Object.defineProperty(g, "__envModule", { value: envModule });
  }
