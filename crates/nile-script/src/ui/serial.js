// @nile/serial: serial ports (COM ports, USB-serial adapters, Bluetooth serial) for scales, card readers, scanners,
// receipt printers and lab gear.
//
//   import serial from "@nile/serial";                      // or: import { list, open } from "@nile/serial"
//   const ports = await serial.list();                      // [{ path, type, vendorId, productId, manufacturer, product, serialNumber }]
//   const port = await serial.open("/dev/tty.usbserial-1", { baudRate: 9600 });
//   port.onData((bytes) => ...);                            // Uint8Array chunks as they arrive
//   await port.write("W\r\n");                              // string (UTF-8), Uint8Array, ArrayBuffer or array of bytes
//   const line = await port.readUntil("\n", { timeout: 2000 });     // bytes up to and including the delimiter
//   await port.close();
//
// open(path, options): baudRate (9600), dataBits (5-8, 8), stopBits (1|2, 1), parity ("none"|"even"|"odd"),
// flowControl ("none"|"hardware"|"software"), timeoutMs (write timeout, 1000).
// Data that arrives while no listener is set waits in a buffer (up to 1 MiB; the oldest bytes go first) for
// read()/readUntil(). port.read(length, { timeout }) resolves with exactly `length` bytes; readUntil(delimiter, { timeout,
// limit }) with everything through the delimiter (a string or bytes); both reject with code SERIAL_TIMEOUT when a
// non-zero `timeout` (ms) passes first. port.discard() empties the buffer.
// port.setSignals({ dataTerminalReady, requestToSend, break }), port.getSignals() -> { clearToSend, dataSetReady,
// ringIndicator, carrierDetect, bytesToRead, bytesToWrite }. port.onClose(cb) runs when the device goes away (cb gets
// the message). Errors carry the system's message with a leading code (ENOENT, EACCES, ENODEV, EINVAL, EIO) and `code`;
// SERIAL_CLOSED and SERIAL_TIMEOUT come from here. On Linux the user may need to be in the `dialout` group.

const call = (op, ...args) => globalThis.__serial(op, ...args);
const KEEP = 1 << 20;

function bytesOf(data) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (Array.isArray(data)) return Uint8Array.from(data);
  throw new TypeError("write a string, Uint8Array, ArrayBuffer or array of bytes");
}

function b64(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function unb64(text) {
  const s = atob(text);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

const coded = (error) => {
  const m = /^([A-Z]{3,}):/.exec(error && error.message ? error.message : "");
  if (m && !error.code) error.code = m[1];
  return error;
};
const run = (op, ...args) => call(op, ...args).catch((e) => { throw coded(e instanceof Error ? e : new Error(String(e))); });
const failure = (code, message) => Object.assign(new Error(message), { code });

export async function list() {
  return JSON.parse(await run("list"));
}

export async function open(path, options = {}) {
  const id = await run("open", JSON.stringify(Object.assign({}, options, { path: String(path) })));
  let state = "open";
  let closing = null;
  let pending = new Uint8Array(0);
  const dataListeners = new Set();
  const closeListeners = new Set();
  const waiters = new Set();

  const lost = (message) => {
    if (state !== "open") return;
    state = "lost";
    for (const cb of closeListeners) { try { cb(message); } catch (e) { console.error(e); } }
    for (const w of [...waiters]) w.fail(failure("EIO", message || "the port closed"));
  };
  // One read is always waiting; each answer is a chunk for the listeners, or the buffer when there are none.
  (async () => {
    while (state === "open") {
      let answer;
      try { answer = await call("read", id, 16384, 0); } catch (e) { lost(e && e.message ? e.message : String(e)); return; }
      if (answer[0] === "E") return;
      if (answer[0] !== "D") continue;
      const chunk = unb64(answer.slice(1));
      if (dataListeners.size) {
        for (const cb of dataListeners) { try { cb(chunk); } catch (e) { console.error(e); } }
      } else {
        const joined = new Uint8Array(pending.length + chunk.length);
        joined.set(pending); joined.set(chunk, pending.length);
        pending = joined.length > KEEP ? joined.subarray(joined.length - KEEP) : joined;
        for (const w of [...waiters]) w.check();
      }
    }
  })();

  const need = () => {
    if (state !== "open") throw failure("SERIAL_CLOSED", "the port is closed");
  };
  const waitFor = (take, what, timeout) => new Promise((resolve, reject) => {
    const w = {
      check() { const got = take(); if (got) { done(); resolve(got); } },
      fail(error) { done(); reject(error); },
    };
    const timer = timeout > 0 ? setTimeout(() => { done(); reject(failure("SERIAL_TIMEOUT", "timed out waiting for " + what)); }, timeout) : null;
    const done = () => { waiters.delete(w); if (timer) clearTimeout(timer); };
    waiters.add(w);
    w.check();
  });
  const cut = (n) => { const out = pending.slice(0, n); pending = pending.subarray(n); return out; };

  return Object.freeze({
    path: String(path),
    get isOpen() { return state === "open"; },
    onData(cb) { dataListeners.add(cb); return () => dataListeners.delete(cb); },
    onClose(cb) { closeListeners.add(cb); return () => closeListeners.delete(cb); },
    async write(data) { need(); return Number(await run("write", id, b64(bytesOf(data)))); },
    read(length, { timeout = 0 } = {}) {
      need();
      return waitFor(() => (pending.length >= length ? cut(length) : null), length + " bytes", timeout);
    },
    readUntil(delimiter, { timeout = 0, limit = KEEP } = {}) {
      need();
      const d = bytesOf(delimiter);
      return waitFor(() => {
        outer: for (let i = 0; i + d.length <= pending.length; i++) {
          for (let j = 0; j < d.length; j++) if (pending[i + j] !== d[j]) continue outer;
          return cut(i + d.length);
        }
        return pending.length >= limit ? cut(limit) : null;
      }, "the delimiter", timeout);
    },
    discard() { pending = new Uint8Array(0); },
    async setSignals(signals) { need(); await run("signals", id, JSON.stringify(signals || {})); },
    async getSignals() { need(); return JSON.parse(await run("status", id)); },
    close() {
      if (closing) return closing;
      for (const w of [...waiters]) w.fail(failure("SERIAL_CLOSED", "the port is closed"));
      const was = state;
      state = "closed";
      closing = was === "open" ? run("close", id).then(() => undefined) : Promise.resolve();
      return closing;
    },
  });
}

export default Object.freeze({ list, open });
