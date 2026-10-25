// @nile/crypto: hashing and encryption helpers over `crypto.subtle`, for the things desktop apps do every day.
//
//   import { hashPassword, verifyPassword, sha256, hmac, encryptText, decryptText, uuid } from "@nile/crypto";
//   const stored = await hashPassword("correct horse");       // "pbkdf2-sha256$600000$<salt>$<hash>": keep this string
//   if (await verifyPassword(typed, stored)) { ... }            // constant-time compare
//   if (needsRehash(stored)) stored = await hashPassword(typed);   // after a successful login, when the cost went up
//   await sha256("text") -> hex          await hmac(key, data) -> hex
//   const sealed = await encryptText("my password", "secret note");    // AES-256-GCM, key from PBKDF2
//   const note = await decryptText("my password", sealed);              // rejects when the password is wrong
//
// Passwords are hashed with PBKDF2-HMAC-SHA256, a random 16 byte salt, 600,000 iterations by default (the current
// OWASP guidance for this algorithm). It runs on a worker thread, so the window stays responsive, but it is meant to
// take a moment (a few hundred milliseconds): don't call it in a loop. `{ iterations }` overrides the cost. The stored
// string carries its own parameters, so raising the default later doesn't break old hashes. Not included: Argon2/scrypt/bcrypt
// (they need native code beyond what ships in Nile today); PBKDF2 at this cost is still an accepted choice.
//
// Everything is also available directly as the standard `crypto.getRandomValues`, `crypto.randomUUID` and
// `crypto.subtle` (digest, HMAC, PBKDF2, AES-GCM).

const subtle = crypto.subtle;
const encoder = new TextEncoder();
const DEFAULT_ITERATIONS = 600000;

const toHex = (buf) => Array.from(new Uint8Array(buf), (b) => (b < 16 ? "0" : "") + b.toString(16)).join("");
const toB64 = (buf) => {
  const u = new Uint8Array(buf);
  let s = "";
  for (let i = 0; i < u.length; i++) s += String.fromCharCode(u[i]);
  return btoa(s);
};
const fromB64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
const bytesOf = (x) => (typeof x === "string" ? encoder.encode(x) : x);

/** Cryptographically secure random bytes. */
export function randomBytes(n) {
  return crypto.getRandomValues(new Uint8Array(n));
}
export function uuid() {
  return crypto.randomUUID();
}
export function hex(buffer) {
  return toHex(buffer);
}
/** SHA-256 (or "SHA-1", "SHA-384", "SHA-512") of text or bytes, as hex. */
export async function digest(data, algorithm) {
  return toHex(await subtle.digest(algorithm || "SHA-256", bytesOf(data)));
}
export function sha256(data) {
  return digest(data, "SHA-256");
}
/** HMAC of `data` with `key` (text or bytes), as hex. */
export async function hmac(key, data, algorithm) {
  const k = await subtle.importKey("raw", bytesOf(key), { name: "HMAC", hash: algorithm || "SHA-256" }, false, ["sign"]);
  return toHex(await subtle.sign("HMAC", k, bytesOf(data)));
}

async function derive(password, salt, iterations, bits) {
  const base = await subtle.importKey("raw", bytesOf(password), "PBKDF2", false, ["deriveBits"]);
  return new Uint8Array(await subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: salt, iterations: iterations }, base, bits));
}

function same(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

/** A string to store for `password`: algorithm, cost, salt and hash. */
export async function hashPassword(password, options) {
  const iterations = (options && options.iterations) || DEFAULT_ITERATIONS;
  const salt = randomBytes(16);
  const hash = await derive(password, salt, iterations, 256);
  return "pbkdf2-sha256$" + iterations + "$" + toB64(salt) + "$" + toB64(hash);
}

/** True when `password` matches a string made by `hashPassword`. A malformed string is simply false. */
export async function verifyPassword(password, stored) {
  const parts = String(stored).split("$");
  if (parts.length !== 4 || parts[0] !== "pbkdf2-sha256") return false;
  const iterations = Number(parts[1]);
  if (!Number.isInteger(iterations) || iterations < 1) return false;
  let salt, want;
  try {
    salt = fromB64(parts[2]);
    want = fromB64(parts[3]);
  } catch (e) {
    return false;
  }
  return same(await derive(password, salt, iterations, want.length * 8), want);
}

/** Whether a stored hash was made with a lower cost than now (rehash it after a successful login). */
export function needsRehash(stored, options) {
  const parts = String(stored).split("$");
  return parts.length !== 4 || parts[0] !== "pbkdf2-sha256" || Number(parts[1]) < ((options && options.iterations) || DEFAULT_ITERATIONS);
}

async function aesKey(password, salt, iterations) {
  const base = await subtle.importKey("raw", bytesOf(password), "PBKDF2", false, ["deriveKey"]);
  return subtle.deriveKey({ name: "PBKDF2", hash: "SHA-256", salt: salt, iterations: iterations }, base, { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
}

/** Encrypt text under a password: "aes256gcm$<iterations>$<salt>$<iv>$<data>". */
export async function encryptText(password, text, options) {
  const iterations = (options && options.iterations) || DEFAULT_ITERATIONS;
  const salt = randomBytes(16), iv = randomBytes(12);
  const key = await aesKey(password, salt, iterations);
  const data = await subtle.encrypt({ name: "AES-GCM", iv: iv }, key, encoder.encode(String(text)));
  return "aes256gcm$" + iterations + "$" + toB64(salt) + "$" + toB64(iv) + "$" + toB64(data);
}

/** The text `encryptText` sealed; rejects (OperationError) when the password is wrong or the data was changed. */
export async function decryptText(password, sealed) {
  const parts = String(sealed).split("$");
  if (parts.length !== 5 || parts[0] !== "aes256gcm") throw new Error("not an encrypted text");
  const key = await aesKey(password, fromB64(parts[2]), Number(parts[1]));
  const plain = await subtle.decrypt({ name: "AES-GCM", iv: fromB64(parts[3]) }, key, fromB64(parts[4]));
  return new TextDecoder().decode(plain);
}
