  // ---- Audio: the Sciter `Audio` class and the web `<audio>` / `new Audio()` -----------
  // Spliced into web.js. One controller per media element; the decoding and the output device
  // are in Rust (nile-audio): `N.audioLoad(src)` registers a file, `N.audioOp(op, id, arg)`
  // drives it, and `__audioTick` (called from the host's tick) reports what ended.
  const auCtl = new WeakMap();    // element -> controller
  const auLive = new Set();       // controllers with a loaded file
  const auFail = (r) => typeof r === "string" && r.charCodeAt(0) === 1 ? r.slice(1) : null;
  const auOp = (c, op, arg = 0) => {
    const r = N.audioOp(op, c.id, arg);
    const e = auFail(r);
    if (e) throw Object.assign(new Error(e), { name: "InvalidStateError" });
    return r;
  };
  const auFire = (el, type) => { try { el.dispatchEvent(new Event(type)); } catch (e) { console.error(e); } };
  const auLater = (el, ...types) => setTimeout(() => types.forEach((t) => auFire(el, t)), 0);

  const auSource = (el) => {
    let src = el.getAttribute("src");
    if (!src) {
      const s = el.querySelector && el.querySelector("source[src]");
      src = s && s.getAttribute("src");
    }
    return src || "";
  };

  const auController = (el) => {
    let c = auCtl.get(el);
    if (!c) {
      c = { el, id: 0, src: "", duration: NaN, error: null, volume: 1, muted: false, loop: false, rate: 1, sciter: false,
            paused: true, ended: false, ends: [], tickAt: 0, wantPlay: false };
      auCtl.set(el, c);
    }
    return c;
  };

  // Load (or reload when `src` changed). Returns true when a file is ready.
  const auLoad = (c) => {
    const src = auSource(c.el);
    if (c.id && c.src === src) return true;
    if (c.id) { N.audioOp("unload", c.id, 0); auLive.delete(c); c.id = 0; }
    c.src = src; c.error = null; c.duration = NaN; c.ended = false; c.paused = true;
    if (!src) return false;
    const r = N.audioLoad(src);
    const fail = auFail(r);
    if (fail) {
      c.error = { code: /not found|no such/i.test(fail) ? 4 : 3, message: fail };
      auLater(c.el, "error");
      return false;
    }
    const [id, duration] = r.split(",");
    c.id = +id; c.duration = duration === "" ? Infinity : +duration;
    auLive.add(c);
    N.audioOp("volume", c.id, c.volume); N.audioOp("muted", c.id, c.muted ? 1 : 0);
    N.audioOp("loop", c.id, c.loop ? 1 : 0); N.audioOp("rate", c.id, c.rate);
    auLater(c.el, "loadstart", "durationchange", "loadedmetadata", "loadeddata", "canplay", "canplaythrough");
    return true;
  };

  const auState = (c) => (c.id ? JSON.parse(N.audioOp("state", c.id, 0)) : null);

  const auPlay = (c) => {
    if (!auLoad(c)) {
      const e = Object.assign(new Error(c.error ? c.error.message : "The media has no supported source."), { name: "NotSupportedError" });
      return Promise.reject(e);
    }
    const was = c.paused || c.ended;
    auOp(c, "play");
    c.paused = false; c.ended = false;
    if (was) auLater(c.el, "play", "playing");
    c.tickAt = 0;
    // Sciter: the promise settles when playback ends; the web: as soon as it has started.
    return c.sciter ? new Promise((resolve) => c.ends.push(resolve)) : Promise.resolve();
  };
  const auPause = (c) => {
    if (!c.id || c.paused) return;
    auOp(c, "pause");
    c.paused = true;
    auLater(c.el, "pause");
  };
  const auSettle = (c) => { const r = c.ends; c.ends = []; r.forEach((f) => f()); };

  // Called by the host on each tick while something plays (and once more when it ends).
  g.__audioTick = (ended) => {
    const now = Date.now();
    for (const c of auLive) {
      if (ended.includes(c.id)) {
        c.paused = true; c.ended = true;
        auLater(c.el, "timeupdate", "pause", "ended");
        auSettle(c);
      } else if (!c.paused && now - c.tickAt >= 250) {
        c.tickAt = now;
        auFire(c.el, "timeupdate");
      }
    }
  };

  const auIs = (el) => el.tagName === "AUDIO";
  const auDefine = (name, get, set) => {
    Object.defineProperty(Element.prototype, name, {
      get() { return auIs(this) ? get(auController(this), this) : this[Symbol.for("nile.auPlain." + name)]; },
      set(v) {
        if (auIs(this) && set) set(auController(this), v, this);
        else Object.defineProperty(this, name, { value: v, writable: true, configurable: true, enumerable: true });
      },
      configurable: true,
    });
  };
  // Properties that other elements already define keep working: only <audio> is redirected.
  const auKeep = (name, get, set) => {
    const old = Object.getOwnPropertyDescriptor(Element.prototype, name);
    Object.defineProperty(Element.prototype, name, {
      get() { return auIs(this) ? get(auController(this), this) : old && old.get ? old.get.call(this) : undefined; },
      set(v) {
        if (auIs(this)) set && set(auController(this), v, this);
        else if (old && old.set) old.set.call(this, v);
        else Object.defineProperty(this, name, { value: v, writable: true, configurable: true, enumerable: true });
      },
      configurable: true,
    });
  };
  auKeep("src", (c, el) => el.getAttribute("src") ?? "", (c, v, el) => { el.setAttribute("src", String(v)); auLoad(c); });
  auDefine("currentSrc", (c) => c.src);
  auDefine("paused", (c) => c.paused);
  auDefine("ended", (c) => c.ended);
  auDefine("duration", (c) => { auLoad(c); return c.duration; });
  auDefine("currentTime", (c) => { auLoad(c); const s = auState(c); return s ? s.position : 0; }, (c, v, el) => {
    if (!auLoad(c)) return;
    auOp(c, "seek", Math.max(0, +v || 0)); c.ended = false;
    auLater(el, "seeking", "seeked", "timeupdate");
  });
  auDefine("progress", (c) => { auLoad(c); const s = auState(c); return s && s.duration > 0 && isFinite(s.duration) ? Math.min(1, s.position / s.duration) : 0; });
  auDefine("volume", (c) => c.volume, (c, v, el) => {
    v = +v;
    if (!(v >= 0 && v <= 1)) throw Object.assign(new Error("The volume must be between 0 and 1"), { name: "IndexSizeError" });
    c.volume = v; if (c.id) auOp(c, "volume", v); auLater(el, "volumechange");
  });
  auDefine("muted", (c) => c.muted, (c, v, el) => { c.muted = !!v; if (c.id) auOp(c, "muted", c.muted ? 1 : 0); auLater(el, "volumechange"); });
  auDefine("loop", (c) => c.loop, (c, v) => { c.loop = !!v; if (c.id) auOp(c, "loop", c.loop ? 1 : 0); });
  auDefine("playbackRate", (c) => c.rate, (c, v, el) => { c.rate = Math.min(4, Math.max(0.25, +v || 1)); if (c.id) auOp(c, "rate", c.rate); auLater(el, "ratechange"); });
  auDefine("defaultPlaybackRate", () => 1);
  auDefine("readyState", (c) => { auLoad(c); return c.id ? 4 : 0; });
  auDefine("networkState", (c) => (c.id ? 1 : c.error ? 3 : 0));
  auDefine("error", (c) => c.error);
  auDefine("seeking", () => false);
  auDefine("preload", () => "auto", () => {});
  auDefine("controls", (c, el) => el.hasAttribute("controls"), (c, v, el) => { v ? el.setAttribute("controls", "") : el.removeAttribute("controls"); });
  auDefine("autoplay", (c, el) => el.hasAttribute("autoplay"), (c, v, el) => { v ? el.setAttribute("autoplay", "") : el.removeAttribute("autoplay"); });
  auDefine("buffered", (c) => ({ length: c.id ? 1 : 0, start: () => 0, end: () => (c.duration === Infinity ? 0 : c.duration) }));
  auDefine("played", (c) => ({ length: 0, start: () => 0, end: () => 0 }));
  auDefine("seekable", (c) => ({ length: c.id ? 1 : 0, start: () => 0, end: () => (c.duration === Infinity ? 0 : c.duration) }));

  // Methods: `play` / `pause` exist on <audio> only (and `load`, `canPlayType`, plus Sciter's
  // `resume` and `stop`, which rewinds and ends a pending `play()` promise).
  const auMethod = (name, fn) => {
    const old = Object.getOwnPropertyDescriptor(Element.prototype, name);
    Object.defineProperty(Element.prototype, name, {
      value(...a) {
        if (auIs(this)) return fn(auController(this), this, ...a);
        if (old && old.value) return old.value.apply(this, a);
        throw new TypeError(name + " is not a function");
      },
      writable: true, configurable: true,
    });
  };
  auMethod("play", (c) => auPlay(c));
  auMethod("pause", (c) => { auPause(c); });
  auMethod("resume", (c) => { auPlay(c).catch((e) => console.error(e)); });
  auMethod("stop", (c, el) => {
    if (c.id) { auOp(c, "stop"); c.paused = true; c.ended = false; auSettle(c); auLater(el, "pause"); }
  });
  auMethod("load", (c) => { if (c.id) { N.audioOp("unload", c.id, 0); auLive.delete(c); c.id = 0; } c.src = "\0"; auLoad(c); });
  auMethod("canPlayType", (c, el, type) => (/^audio\/(mpeg|mp3|ogg|wav|x-wav|wave|flac|x-flac|aac|mp4|x-m4a|vorbis)/i.test(String(type)) ? "probably" : ""));

  // `new Audio(url)` is an <audio> element; `Audio.load(url)` is Sciter's: a promise of one whose
  // `play()` settles when the sound ends. The page's own <audio autoplay> starts at load.
  const Audio = function Audio(src) {
    const el = document.createElement("audio");
    if (src !== undefined) el.setAttribute("src", String(src));
    return el;
  };
  Audio.load = (url) => new Promise((resolve, reject) => {
    const el = document.createElement("audio");
    el.setAttribute("src", String(url));
    const c = auController(el);
    c.sciter = true;
    if (!auLoad(c)) reject(new Error(c.error ? c.error.message : "404")); else resolve(el);
  });
  Object.defineProperty(Audio, Symbol.hasInstance, { value: (o) => !!o && o.tagName === "AUDIO" });
  g.Audio = Audio;
  g.HTMLAudioElement = class HTMLAudioElement { static [Symbol.hasInstance](o) { return !!o && o.tagName === "AUDIO"; } };
  g.HTMLMediaElement = g.HTMLAudioElement;
  document.addEventListener("DOMContentLoaded", () => {
    for (const el of document.querySelectorAll("audio[autoplay]")) auPlay(auController(el)).catch((e) => console.error(e));
  });
