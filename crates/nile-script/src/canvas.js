  // ---- <canvas> and the 2D context ----------------------------------------------------
  // Spliced into web.js (so it shares `bytesToB64`/`b64ToBytes`). The drawing itself is in Rust
  // (nile-canvas): every method is one `N.canvasCall(id, operation, numbers, text)`.
  const cvRun = (id, op, args = [], text = "") => {
    const r = N.canvasCall(id, op, args, text);
    if (typeof r === "string" && r.charCodeAt(0) === 1) {
      const [name, ...rest] = r.slice(1).split(": ");
      const e = new Error(rest.join(": ") || name);
      e.name = rest.length ? name : "Error";
      throw e;
    }
    return r;
  };
  const cvNaN = Number.NaN;
  const cvFinite = (...v) => v.every((x) => Number.isFinite(x));
  const cvNum = (v) => Number(v);

  // The element's backing store, made on first use and remembered in an internal attribute (the
  // renderer finds it there).
  const cvId = (el) => {
    let id = parseInt(el.getAttribute("nile-canvas"), 10);
    if (!(id > 0)) {
      id = N.canvasCreate(el._cvWidth(), el._cvHeight());
      if (!(id > 0)) throw new Error("InvalidStateError: the canvas is too large");
      el.setAttribute("nile-canvas", String(id));
    }
    return id;
  };

  class CanvasGradient {
    constructor(id) { Object.defineProperty(this, "_id", { value: id }); }
    addColorStop(offset, color) { cvRun(0, "addColorStop", [this._id, cvNum(offset)], String(color)); }
  }
  class CanvasPattern { constructor(id) { Object.defineProperty(this, "_id", { value: id }); } }
  g.CanvasGradient = CanvasGradient;
  g.CanvasPattern = CanvasPattern;

  class ImageData {
    constructor(a, b, c) {
      if (typeof a === "number") {
        this.width = a >>> 0; this.height = b >>> 0;
        this.data = new Uint8ClampedArray(this.width * this.height * 4);
      } else {
        this.data = a; this.width = b >>> 0;
        this.height = c === undefined ? (a.length / 4 / this.width) | 0 : c >>> 0;
      }
      if (!this.width || !this.height) throw Object.assign(new Error("The source width and height must be nonzero"), { name: "IndexSizeError" });
      this.colorSpace = "srgb";
    }
  }
  g.ImageData = ImageData;

  // Path2D: an id in the native path table; the same construction methods as the context.
  const CV_PATH_OPS = ["closePath", "moveTo", "lineTo", "bezierCurveTo", "quadraticCurveTo", "arcTo", "rect", "arc", "ellipse"];
  const cvRadii = (r) => {
    const list = Array.isArray(r) ? r : [r];
    const v = list.map((x) => (typeof x === "object" && x ? cvNum(x.x ?? x.y ?? 0) : cvNum(x)));
    if (v.length === 1) return [v[0], v[0], v[0], v[0]];
    if (v.length === 2) return [v[0], v[1], v[0], v[1]];
    if (v.length === 3) return [v[0], v[1], v[2], v[1]];
    return v.slice(0, 4);
  };
  class Path2D {
    constructor(init) {
      if (init instanceof Path2D) {
        Object.defineProperty(this, "_id", { value: parseInt(cvRun(0, "newPath"), 10) });
        cvRun(0, "pathOp", [init._id, 1, 0, 0, 1, 0, 0], this._id + ":addPath");
      } else if (typeof init === "string") {
        Object.defineProperty(this, "_id", { value: parseInt(cvRun(0, "pathFromSvg", [], init), 10) });
      } else {
        Object.defineProperty(this, "_id", { value: parseInt(cvRun(0, "newPath"), 10) });
      }
    }
    addPath(path, m) {
      const t = m ? [m.a, m.b, m.c, m.d, m.e, m.f] : [1, 0, 0, 1, 0, 0];
      cvRun(0, "pathOp", [path._id, ...t], this._id + ":addPath");
    }
    roundRect(x, y, w, h, r = 0) { cvRun(0, "pathOp", [cvNum(x), cvNum(y), cvNum(w), cvNum(h), ...radiiOf(r)], this._id + ":roundRect"); }
  }
  for (const op of CV_PATH_OPS) {
    Path2D.prototype[op] = function (...args) {
      if (op === "ellipse" || op === "arc") args = args.map((v, i) => (i === (op === "arc" ? 5 : 7) ? (v ? 1 : 0) : cvNum(v)));
      else args = args.map(cvNum);
      if (args.some((v) => Number.isNaN(v))) return;
      cvRun(0, "pathOp", args, this._id + ":" + op);
    };
  }
  g.Path2D = Path2D;

  const CV_DEFAULTS = () => ({
    fillStyle: "#000000", strokeStyle: "#000000", globalAlpha: 1, globalCompositeOperation: "source-over",
    lineWidth: 1, lineCap: "butt", lineJoin: "miter", miterLimit: 10, lineDashOffset: 0, lineDash: [],
    shadowBlur: 0, shadowColor: "rgba(0, 0, 0, 0)", shadowOffsetX: 0, shadowOffsetY: 0,
    imageSmoothingEnabled: true, imageSmoothingQuality: "low",
    font: "10px sans-serif", textAlign: "start", textBaseline: "alphabetic", direction: "ltr",
    fontKerning: "auto", letterSpacing: "0px", wordSpacing: "0px", textRendering: "auto", filter: "none",
  });

  class CanvasRenderingContext2D {
    constructor(el) {
      Object.defineProperty(this, "_el", { value: el });
      Object.defineProperty(this, "_s", { value: CV_DEFAULTS(), writable: true });
      Object.defineProperty(this, "_stack", { value: [] });
    }
    get canvas() { return this._el; }
    get _id() { return cvId(this._el); }
    _reset() { this._s = CV_DEFAULTS(); this._stack.length = 0; }
    _call(op, args, text) { return cvRun(this._id, op, args, text); }

    save() { this._stack.push({ ...this._s, lineDash: this._s.lineDash.slice() }); this._call("save"); }
    restore() { const s = this._stack.pop(); if (s) { this._s = s; this._call("restore"); } }
    reset() { this._call("reset"); this._reset(); }
    isContextLost() { return false; }

    scale(x, y) { if (cvFinite(cvNum(x), cvNum(y))) this._call("scale", [cvNum(x), cvNum(y)]); }
    rotate(a) { if (cvFinite(cvNum(a))) this._call("rotate", [cvNum(a)]); }
    translate(x, y) { if (cvFinite(cvNum(x), cvNum(y))) this._call("translate", [cvNum(x), cvNum(y)]); }
    transform(a, b, c, d, e, f) { const v = [a, b, c, d, e, f].map(cvNum); if (cvFinite(...v)) this._call("transform", v); }
    setTransform(a, b, c, d, e, f) {
      let v = [a, b, c, d, e, f];
      if (a && typeof a === "object") v = [a.a, a.b, a.c, a.d, a.e, a.f];
      else if (a === undefined) v = [1, 0, 0, 1, 0, 0];
      v = v.map(cvNum);
      if (cvFinite(...v)) this._call("setTransform", v);
    }
    resetTransform() { this._call("resetTransform"); }
    getTransform() {
      const [a, b, c, d, e, f] = JSON.parse(this._call("getTransform"));
      const m = { a, b, c, d, e, f, m11: a, m12: b, m21: c, m22: d, m41: e, m42: f, is2D: true, isIdentity: a === 1 && b === 0 && c === 0 && d === 1 && e === 0 && f === 0 };
      return typeof g.DOMMatrix === "function" ? new g.DOMMatrix([a, b, c, d, e, f]) : m;
    }

    clearRect(x, y, w, h) { const v = [x, y, w, h].map(cvNum); if (cvFinite(...v)) this._call("clearRect", v); }
    fillRect(x, y, w, h) { const v = [x, y, w, h].map(cvNum); if (cvFinite(...v)) this._call("fillRect", v); }
    strokeRect(x, y, w, h) { const v = [x, y, w, h].map(cvNum); if (cvFinite(...v)) this._call("strokeRect", v); }

    beginPath() { this._call("beginPath"); }
    rect(x, y, w, h) { const v = [x, y, w, h].map(cvNum); if (cvFinite(...v)) this._call("rect", v); }
    roundRect(x, y, w, h, r = 0) { const v = [x, y, w, h].map(cvNum); if (cvFinite(...v)) this._call("roundRect", [...v, ...radiiOf(r)]); }
    createLinearGradient(x0, y0, x1, y1) { return new CanvasGradient(parseInt(cvRun(0, "createLinearGradient", [x0, y0, x1, y1].map(cvNum)), 10)); }
    createRadialGradient(x0, y0, r0, x1, y1, r1) { return new CanvasGradient(parseInt(cvRun(0, "createRadialGradient", [x0, y0, r0, x1, y1, r1].map(cvNum)), 10)); }
    createConicGradient(a, x, y) { return new CanvasGradient(parseInt(cvRun(0, "createConicGradient", [a, x, y].map(cvNum)), 10)); }
    createPattern(image, repetition = "repeat") {
      const src = cvSource(image);
      if (!src) return null;
      const id = cvRun(0, "createPattern", [], src + "|" + (repetition ?? "repeat"));
      return id ? new CanvasPattern(parseInt(id, 10)) : null;
    }

    _rule(a, b) { const rule = a instanceof Path2D ? b : a; return [rule === "evenodd" ? 1 : 0, a instanceof Path2D ? String(a._id) : ""]; }
    fill(a, b) { const [rule, p] = this._rule(a, b); this._call("fill", [rule], p); }
    stroke(path) { this._call("stroke", [], path instanceof Path2D ? String(path._id) : ""); }
    clip(a, b) { const [rule, p] = this._rule(a, b); this._call("clip", [rule], p); }
    isPointInPath(a, b, c, d) {
      const path = a instanceof Path2D ? a : null;
      const [x, y, rule] = path ? [b, c, d] : [a, b, c];
      return this._call("isPointInPath", [cvNum(x), cvNum(y), rule === "evenodd" ? 1 : 0], path ? String(path._id) : "") === "1";
    }
    isPointInStroke(a, b, c) {
      const path = a instanceof Path2D ? a : null;
      const [x, y] = path ? [b, c] : [a, b];
      return this._call("isPointInStroke", [cvNum(x), cvNum(y)], path ? String(path._id) : "") === "1";
    }

    fillText(text, x, y, max) { this._text("fillText", text, x, y, max); }
    strokeText(text, x, y, max) { this._text("strokeText", text, x, y, max); }
    _text(op, text, x, y, max) {
      const v = [cvNum(x), cvNum(y), max === undefined ? cvNaN : cvNum(max)];
      if (cvFinite(v[0], v[1])) this._call(op, v, String(text));
    }
    measureText(text) {
      const m = JSON.parse(this._call("measureText", [], String(text)));
      return Object.assign(Object.create({ constructor: g.TextMetrics }), m);
    }

    drawImage(image, ...a) {
      const src = cvSource(image);
      if (!src) { if (image && typeof image === "object") return; throw new TypeError("drawImage: the first argument is not an image"); }
      const v = a.map(cvNum);
      let args;
      if (v.length === 2) args = [cvNaN, cvNaN, cvNaN, cvNaN, v[0], v[1], cvNaN, cvNaN];
      else if (v.length === 4) args = [cvNaN, cvNaN, cvNaN, cvNaN, v[0], v[1], v[2], v[3]];
      else if (v.length === 8) args = v;
      else throw new TypeError("drawImage: " + v.length + " arguments given");
      if (args.every((x) => Number.isNaN(x) || Number.isFinite(x))) this._call("drawImage", args, src);
    }
    createImageData(a, b) { return a instanceof ImageData ? new ImageData(a.width, a.height) : new ImageData(a, b); }
    getImageData(x, y, w, h) {
      const [sx, sy, sw, sh] = [x, y, w, h].map((v) => Math.trunc(cvNum(v)));
      const b64 = this._call("getImageData", [sx, sy, sw, sh]);
      return new ImageData(new Uint8ClampedArray(b64ToBytes(b64).buffer), Math.abs(sw), Math.abs(sh));
    }
    putImageData(img, dx, dy, ...dirty) {
      let { data, width, height } = img;
      dx = Math.trunc(cvNum(dx)); dy = Math.trunc(cvNum(dy));
      if (dirty.length === 4) {
        let [rx, ry, rw, rh] = dirty.map((v) => Math.trunc(cvNum(v)));
        if (rw < 0) { rx += rw; rw = -rw; }
        if (rh < 0) { ry += rh; rh = -rh; }
        rx = Math.max(rx, 0); ry = Math.max(ry, 0);
        rw = Math.min(rw, width - rx); rh = Math.min(rh, height - ry);
        if (rw <= 0 || rh <= 0) return;
        const out = new Uint8Array(rw * rh * 4);
        for (let row = 0; row < rh; row++) out.set(data.subarray(((ry + row) * width + rx) * 4, ((ry + row) * width + rx + rw) * 4), row * rw * 4);
        data = out; dx += rx; dy += ry; width = rw; height = rh;
      }
      this._call("putImageData", [width, height, dx, dy], bytesToB64(new Uint8Array(data.buffer, data.byteOffset, width * height * 4)));
    }
    setLineDash(list) {
      const v = Array.from(list, cvNum);
      if (!v.every((x) => Number.isFinite(x) && x >= 0)) return;
      this._s.lineDash = v;
      cvRun(this._id, "setLineDash", v);
    }
    getLineDash() { return this._s.lineDash.length % 2 ? this._s.lineDash.concat(this._s.lineDash) : this._s.lineDash.slice(); }
  }

  // Properties that are mirrored here (so a getter reads back what was accepted) and set natively.
  const cvNumeric = (name, ok) => ({
    get() { return this._s[name]; },
    set(v) { v = cvNum(v); if (Number.isFinite(v) && ok(v)) { this._s[name] = v; this._call("set:" + name, [v]); } },
    enumerable: true,
  });
  const cvKeyword = (name) => ({
    get() { return this._s[name]; },
    set(v) { const r = this._call("set:" + name, [], String(v)); if (r !== "") this._s[name] = r; },
    enumerable: true,
  });
  const cvStored = (name) => ({ get() { return this._s[name]; }, set(v) { this._s[name] = v; }, enumerable: true });
  const cvStyle = (name) => ({
    get() { return this._s[name]; },
    set(v) {
      if (v instanceof CanvasGradient) { this._call("set:" + name, [], "grad:" + v._id); this._s[name] = v; }
      else if (v instanceof CanvasPattern) { this._call("set:" + name, [], "pat:" + v._id); this._s[name] = v; }
      else { const r = this._call("set:" + name, [], String(v)); if (r !== "") this._s[name] = r; }
    },
    enumerable: true,
  });
  Object.defineProperties(CanvasRenderingContext2D.prototype, {
    fillStyle: cvStyle("fillStyle"),
    strokeStyle: cvStyle("strokeStyle"),
    globalAlpha: cvNumeric("globalAlpha", (v) => v >= 0 && v <= 1),
    globalCompositeOperation: cvKeyword("globalCompositeOperation"),
    lineWidth: cvNumeric("lineWidth", (v) => v > 0),
    lineCap: cvKeyword("lineCap"),
    lineJoin: cvKeyword("lineJoin"),
    miterLimit: cvNumeric("miterLimit", (v) => v > 0),
    lineDashOffset: cvNumeric("lineDashOffset", () => true),
    shadowBlur: cvNumeric("shadowBlur", (v) => v >= 0),
    shadowColor: cvKeyword("shadowColor"),
    shadowOffsetX: cvNumeric("shadowOffsetX", () => true),
    shadowOffsetY: cvNumeric("shadowOffsetY", () => true),
    imageSmoothingEnabled: {
      get() { return this._s.imageSmoothingEnabled; },
      set(v) { this._s.imageSmoothingEnabled = !!v; this._call("set:imageSmoothingEnabled", [v ? 1 : 0]); },
      enumerable: true,
    },
    imageSmoothingQuality: cvStored("imageSmoothingQuality"),
    font: cvKeyword("font"),
    textAlign: cvKeyword("textAlign"),
    textBaseline: cvKeyword("textBaseline"),
    direction: cvStored("direction"),
    fontKerning: cvStored("fontKerning"),
    letterSpacing: cvStored("letterSpacing"),
    wordSpacing: cvStored("wordSpacing"),
    textRendering: cvStored("textRendering"),
    filter: cvStored("filter"),
  });
  for (const op of CV_PATH_OPS) {
    CanvasRenderingContext2D.prototype[op] = function (...args) {
      if (op === "ellipse" || op === "arc") args = args.map((v, i) => (i === (op === "arc" ? 5 : 7) ? (v ? 1 : 0) : cvNum(v)));
      else args = args.map(cvNum);
      if (args.some((v) => Number.isNaN(v))) return;
      this._call(op, args);
    };
  }
  g.CanvasRenderingContext2D = CanvasRenderingContext2D;
  g.TextMetrics = class TextMetrics {};

  // What `drawImage`/`createPattern` take: an <img> (by its src), a canvas, an Image.
  const cvSource = (image) => {
    if (!image || typeof image !== "object" || !image.tagName) return null;
    if (image.tagName === "CANVAS") return "cv:" + cvId(image);
    if (image.tagName === "IMG") { const s = image.getAttribute("src"); return s ? "img:" + s : null; }
    return null;
  };

  // `<canvas>` and `<img>` size and the canvas methods, on every element (they only act on those tags).
  const CV_W = 300, CV_H = 150;
  const cvDim = (el, name, dflt) => { const v = parseInt(el.getAttribute(name), 10); return Number.isFinite(v) && v >= 0 ? v : dflt; };
  Object.defineProperties(Element.prototype, {
    _cvWidth: { value() { return cvDim(this, "width", CV_W); } },
    _cvHeight: { value() { return cvDim(this, "height", CV_H); } },
    width: {
      get() {
        if (this.tagName === "CANVAS") return this._cvWidth();
        if (this.tagName === "IMG") return cvDim(this, "width", this.naturalWidth);
        return undefined;
      },
      set(v) {
        const tag = this.tagName;
        if (tag === "CANVAS" || tag === "IMG") {
          this.setAttribute("width", String(Math.max(0, Math.trunc(cvNum(v)) || 0)));
          if (tag === "CANVAS") this._cvResize();
        } else Object.defineProperty(this, "width", { value: v, writable: true, configurable: true, enumerable: true });
      },
      configurable: true,
    },
    height: {
      get() {
        if (this.tagName === "CANVAS") return this._cvHeight();
        if (this.tagName === "IMG") return cvDim(this, "height", this.naturalHeight);
        return undefined;
      },
      set(v) {
        const tag = this.tagName;
        if (tag === "CANVAS" || tag === "IMG") {
          this.setAttribute("height", String(Math.max(0, Math.trunc(cvNum(v)) || 0)));
          if (tag === "CANVAS") this._cvResize();
        } else Object.defineProperty(this, "height", { value: v, writable: true, configurable: true, enumerable: true });
      },
      configurable: true,
    },
    // Changing a canvas's size clears it and resets its context.
    _cvResize: {
      value() {
        const id = parseInt(this.getAttribute("nile-canvas"), 10);
        if (id > 0) N.canvasResize(id, this._cvWidth(), this._cvHeight());
        if (this._ctx2d) this._ctx2d._reset();
      },
    },
    getContext: {
      value(type) {
        if (this.tagName !== "CANVAS") throw new TypeError("getContext is not a function");
        if (String(type) !== "2d") return null;
        if (!this._ctx2d) { cvId(this); Object.defineProperty(this, "_ctx2d", { value: new CanvasRenderingContext2D(this), writable: true }); }
        return this._ctx2d;
      },
    },
    toDataURL: {
      value(type = "image/png", quality) {
        if (this.tagName !== "CANVAS") throw new TypeError("toDataURL is not a function");
        return cvRun(cvId(this), "toDataURL", typeof quality === "number" ? [quality] : [], String(type));
      },
    },
    toBlob: {
      value(callback, type = "image/png", quality) {
        const url = this.toDataURL(type, quality);
        const mime = url.slice(5, url.indexOf(";"));
        const bytes = b64ToBytes(url.slice(url.indexOf(",") + 1));
        setTimeout(() => callback(new Blob([bytes], { type: mime })), 0);
      },
    },
  });
  g.OffscreenCanvas = class OffscreenCanvas {
    constructor(w, h) {
      const el = document.createElement("canvas");
      el.width = w; el.height = h;
      return el;
    }
  };

  // <img>: `src` as an attribute that loads (the `load`/`error` event follows), natural size and `Image`.
  const cvImageInfo = (el) => {
    const src = el.getAttribute("src");
    if (!src) return null;
    const r = cvRun(0, "imageSize", [], "img:" + src);
    return r ? JSON.parse(r) : null;
  };
  Object.defineProperties(Element.prototype, {
    naturalWidth: { get() { const i = this.tagName === "IMG" ? cvImageInfo(this) : null; return i ? i[0] : 0; }, configurable: true },
    naturalHeight: { get() { const i = this.tagName === "IMG" ? cvImageInfo(this) : null; return i ? i[1] : 0; }, configurable: true },
    complete: { get() { return this.tagName !== "IMG" || !this.getAttribute("src") || cvImageInfo(this) !== null; }, configurable: true },
    src: {
      get() { return this.tagName === "IMG" ? this.getAttribute("src") ?? "" : undefined; },
      set(v) {
        if (this.tagName !== "IMG") { Object.defineProperty(this, "src", { value: v, writable: true, configurable: true, enumerable: true }); return; }
        this.setAttribute("src", String(v));
        setTimeout(() => this.dispatchEvent(new Event(cvImageInfo(this) ? "load" : "error")), 0);
      },
      configurable: true,
    },
  });
  g.Image = function Image(w, h) {
    const el = document.createElement("img");
    if (w !== undefined) el.setAttribute("width", String(w));
    if (h !== undefined) el.setAttribute("height", String(h));
    return el;
  };
  // `canvas instanceof HTMLCanvasElement` (there is one Element class; the tag decides).
  g.HTMLCanvasElement = class HTMLCanvasElement { static [Symbol.hasInstance](o) { return !!o && o.tagName === "CANVAS"; } };
  g.HTMLImageElement = class HTMLImageElement { static [Symbol.hasInstance](o) { return !!o && o.tagName === "IMG"; } };
