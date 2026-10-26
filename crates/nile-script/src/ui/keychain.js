// @nile/keychain: keep secrets in the operating system's credential store instead of your own files.
//
//   import { keychain } from "@nile/keychain";
//   keychain.service = "SoftSale";                          // your app's name: one place for all its secrets
//   await keychain.set("smtp-password", "s3cret");           // account name, secret
//   const pw = await keychain.get("smtp-password");          // the text, or null when nothing is stored
//   await keychain.remove("smtp-password");                  // true when something was removed
//   await keychain.setBytes("key", new Uint8Array([1, 2]));  // binary secrets; getBytes returns a Uint8Array or null
//
// macOS uses the Keychain, Windows the Credential Manager, Linux the Secret Service (GNOME Keyring, KWallet) over
// D-Bus. Everything returns a Promise because the system may stop to ask the user to unlock the store. Set
// `keychain.service` once (or pass `{ service }` to a call): without it the program's file name is used, which is shared by
// every Nile app run with the same `nile` binary. `keychain.available()` resolves to true when a store is usable and
// false (with `keychain.reason()` explaining) on a machine with no keyring service. Calls reject with an Error carrying
// the system's message, for example when the user denies access. Secrets are protected by the OS for this user account; other
// programs running as the same user may be able to read them, and a script of yours can read what it stores.

const call = (op, ...args) => globalThis.__keychain(op, ...args);

let service = "";
let lastStatus = "";

function name(options) {
  const s = (options && options.service) || service;
  if (s) return String(s);
  const exe = (typeof process !== "undefined" && process.argv0) || "";
  return exe || "nile-app";
}

export const keychain = {
  get service() {
    return service;
  },
  set service(v) {
    service = String(v || "");
  },
  /** Store a text secret under `account` (replacing any earlier one). */
  async set(account, secret, options) {
    await call("set", name(options), String(account), "text", String(secret));
  },
  /** The text stored under `account`, or null. */
  async get(account, options) {
    const r = await call("get", name(options), String(account), "text");
    return r.charAt(0) === "V" ? r.slice(1) : null;
  },
  async setBytes(account, bytes, options) {
    const u = ArrayBuffer.isView(bytes) ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength) : new Uint8Array(bytes);
    let s = "";
    for (let i = 0; i < u.length; i++) s += String.fromCharCode(u[i]);
    await call("set", name(options), String(account), "bytes", btoa(s));
  },
  async getBytes(account, options) {
    const r = await call("get", name(options), String(account), "bytes");
    if (r.charAt(0) !== "B") return null;
    return Uint8Array.from(atob(r.slice(1)), (c) => c.charCodeAt(0));
  },
  /** Delete the secret under `account`; true when there was one. */
  async remove(account, options) {
    return (await call("remove", name(options), String(account))) === "true";
  },
  /** Whether a credential store is usable here. */
  async available() {
    lastStatus = await call("status");
    return lastStatus === "ok";
  },
  /** Why the last `available()` said no. */
  reason() {
    return lastStatus === "ok" ? "" : lastStatus;
  },
};

export default keychain;
