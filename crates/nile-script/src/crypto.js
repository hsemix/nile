// Web Crypto, the part a desktop app needs: `crypto.getRandomValues`, `crypto.randomUUID` and `crypto.subtle` with
// digest (SHA-1/256/384/512), HMAC (generateKey, importKey, sign, verify), PBKDF2 (deriveBits, deriveKey) and AES-GCM
// (generateKey, importKey, encrypt, decrypt), all over the native `cryptoCall` (ring). Keys are "raw" only (no JWK,
// no public-key algorithms). Spliced into web.js's block, so `N`, `bytesToB64`, `b64ToBytes` and `toBytes` are in scope.
{
  const cryptoRun = (op, ...a) => {
    const r = N.cryptoCall(op, a.map(String));
    if (r.startsWith("E:")) {
      const m = r.slice(2);
      const i = m.indexOf(": ");
      const name = i > 0 && /^[A-Za-z]+Error$/.test(m.slice(0, i)) ? m.slice(0, i) : "OperationError";
      const e = new Error(i > 0 && name === m.slice(0, i) ? m.slice(i + 2) : m);
      e.name = name;
      throw e;
    }
    return r.slice(2);
  };
  // The heavy operations run on a worker thread (`sysAsync` -> `sys::start`), so hashing a big buffer or a slow PBKDF2 doesn't freeze the page.
  const cryptoJob = (op, ...a) => sysAsync("crypto:" + op, ...a).catch((e) => {
    const m = String(e && e.message);
    const i = m.indexOf(": ");
    const name = i > 0 && /^[A-Za-z]+Error$/.test(m.slice(0, i)) ? m.slice(0, i) : "OperationError";
    throw Object.assign(new Error(name === m.slice(0, i) ? m.slice(i + 2) : m), { name });
  });
  const dataOf = (d) => bytesToB64(toBytes(d));
  const buffer = (b64) => { const u = b64ToBytes(b64); return u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength); };
  const algName = (a) => String(typeof a === "string" ? a : a && a.name).toUpperCase();
  const hashName = (h) => {
    const n = String(typeof h === "string" ? h : h && h.name).toUpperCase();
    if (!/^SHA-(1|256|384|512)$/.test(n)) throw Object.assign(new Error("unsupported hash: " + n), { name: "NotSupportedError" });
    return n;
  };
  const fail = (name, message) => Object.assign(new Error(message), { name });
  const KEYS = new WeakMap();
  class CryptoKey {
    constructor() { throw new TypeError("Illegal constructor"); }
    get [Symbol.toStringTag]() { return "CryptoKey"; }
  }
  const makeKey = (raw, algorithm, extractable, usages) => {
    const k = Object.create(CryptoKey.prototype);
    Object.defineProperties(k, {
      type: { value: "secret", enumerable: true },
      extractable: { value: !!extractable, enumerable: true },
      algorithm: { value: Object.freeze(algorithm), enumerable: true },
      usages: { value: Object.freeze(Array.from(usages || [])), enumerable: true },
    });
    KEYS.set(k, raw);
    return k;
  };
  const rawOf = (key, name, usage) => {
    const raw = KEYS.get(key);
    if (!raw) throw new TypeError("not a CryptoKey");
    if (key.algorithm.name.toUpperCase() !== name) throw fail("InvalidAccessError", "the key is not a " + name + " key");
    if (usage && !key.usages.includes(usage)) throw fail("InvalidAccessError", "the key cannot be used to " + usage);
    return raw;
  };
  const blockBits = (hash) => (hash === "SHA-384" || hash === "SHA-512" ? 1024 : 512);
  const pbkdf2Bits = (params, raw, bits) => {
    if (!params.salt) throw new TypeError("PBKDF2 needs a salt");
    return cryptoJob("pbkdf2", hashName(params.hash), raw, dataOf(params.salt), params.iterations | 0, bits);
  };
  // Keys for a derived algorithm ({ name: "AES-GCM", length } or { name: "HMAC", hash, length }).
  const keyFromBits = (raw, algorithm, extractable, usages) => {
    const name = algName(algorithm);
    if (name === "AES-GCM") {
      const len = (b64ToBytes(raw).length) * 8;
      if (len !== 128 && len !== 256) throw fail("OperationError", "AES-GCM keys are 128 or 256 bits");
      return makeKey(raw, { name: "AES-GCM", length: len }, extractable, usages);
    }
    if (name === "HMAC") return makeKey(raw, { name: "HMAC", hash: { name: hashName(algorithm.hash) }, length: b64ToBytes(raw).length * 8 }, extractable, usages);
    if (name === "PBKDF2") return makeKey(raw, { name: "PBKDF2" }, false, usages);
    throw fail("NotSupportedError", "unsupported algorithm: " + name);
  };
  const subtle = {
    async digest(algorithm, data) { return buffer(await cryptoJob("digest", hashName(algorithm), dataOf(data))); },
    async importKey(format, keyData, algorithm, extractable, usages) {
      if (format !== "raw") throw fail("NotSupportedError", "only the raw key format is supported");
      return keyFromBits(dataOf(keyData), algorithm, extractable, usages);
    },
    async exportKey(format, key) {
      if (format !== "raw") throw fail("NotSupportedError", "only the raw key format is supported");
      const raw = KEYS.get(key);
      if (!raw) throw new TypeError("not a CryptoKey");
      if (!key.extractable) throw fail("InvalidAccessError", "the key is not extractable");
      return buffer(raw);
    },
    async generateKey(algorithm, extractable, usages) {
      const name = algName(algorithm);
      if (name === "AES-GCM") {
        const length = algorithm.length || 256;
        if (length !== 128 && length !== 256) throw fail("OperationError", "AES-GCM keys are 128 or 256 bits");
        return makeKey(cryptoRun("random", length / 8), { name: "AES-GCM", length }, extractable, usages);
      }
      if (name === "HMAC") {
        const hash = hashName(algorithm.hash);
        const length = algorithm.length || blockBits(hash);
        return makeKey(cryptoRun("random", Math.ceil(length / 8)), { name: "HMAC", hash: { name: hash }, length }, extractable, usages);
      }
      throw fail("NotSupportedError", "unsupported algorithm: " + name);
    },
    async sign(algorithm, key, data) {
      const raw = rawOf(key, "HMAC", "sign");
      return buffer(await cryptoJob("hmac", key.algorithm.hash.name, raw, dataOf(data)));
    },
    async verify(algorithm, key, signature, data) {
      const raw = rawOf(key, "HMAC", "verify");
      return (await cryptoJob("hmac_verify", key.algorithm.hash.name, raw, dataOf(data), dataOf(signature))) === "true";
    },
    async deriveBits(algorithm, baseKey, length) {
      if (algName(algorithm) !== "PBKDF2") throw fail("NotSupportedError", "only PBKDF2 can derive bits");
      return buffer(await pbkdf2Bits(algorithm, rawOf(baseKey, "PBKDF2", "deriveBits"), length));
    },
    async deriveKey(algorithm, baseKey, derivedAlgorithm, extractable, usages) {
      if (algName(algorithm) !== "PBKDF2") throw fail("NotSupportedError", "only PBKDF2 can derive keys");
      const raw = rawOf(baseKey, "PBKDF2", "deriveKey");
      const bits = derivedAlgorithm.length || (algName(derivedAlgorithm) === "HMAC" ? blockBits(hashName(derivedAlgorithm.hash)) : 256);
      return keyFromBits(await pbkdf2Bits(algorithm, raw, bits), derivedAlgorithm, extractable, usages);
    },
    async encrypt(algorithm, key, data) {
      if (algName(algorithm) !== "AES-GCM") throw fail("NotSupportedError", "only AES-GCM encrypts");
      if (algorithm.tagLength && algorithm.tagLength !== 128) throw fail("NotSupportedError", "only 128 bit tags");
      return buffer(await cryptoJob("aes_gcm_seal", rawOf(key, "AES-GCM", "encrypt"), dataOf(algorithm.iv), dataOf(data), algorithm.additionalData ? dataOf(algorithm.additionalData) : ""));
    },
    async decrypt(algorithm, key, data) {
      if (algName(algorithm) !== "AES-GCM") throw fail("NotSupportedError", "only AES-GCM decrypts");
      return buffer(await cryptoJob("aes_gcm_open", rawOf(key, "AES-GCM", "decrypt"), dataOf(algorithm.iv), dataOf(data), algorithm.additionalData ? dataOf(algorithm.additionalData) : ""));
    },
  };
  const INT_ARRAYS = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "BigInt64Array", "BigUint64Array"];
  const crypto = {
    subtle,
    // Fills an integer typed array in place with random bytes from the system's secure source.
    getRandomValues(array) {
      if (!array || !ArrayBuffer.isView(array) || !INT_ARRAYS.includes(array.constructor.name)) throw fail("TypeMismatchError", "getRandomValues needs an integer typed array");
      if (array.byteLength > 65536) throw fail("QuotaExceededError", "at most 65536 bytes at a time");
      if (array.byteLength) new Uint8Array(array.buffer, array.byteOffset, array.byteLength).set(b64ToBytes(cryptoRun("random", array.byteLength)));
      return array;
    },
    randomUUID() {
      const b = b64ToBytes(cryptoRun("random", 16));
      b[6] = (b[6] & 0x0f) | 0x40;
      b[8] = (b[8] & 0x3f) | 0x80;
      const h = Array.from(b, (x) => (x < 16 ? "0" : "") + x.toString(16)).join("");
      return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20);
    },
  };
  Object.defineProperty(g, "crypto", { value: crypto, configurable: true, writable: true });
  Object.defineProperty(g, "CryptoKey", { value: CryptoKey, configurable: true, writable: true });
}
