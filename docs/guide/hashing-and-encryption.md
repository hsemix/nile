# Hashing, passwords and encryption

Nile gives scripts the standard Web Crypto API, backed by the Rust `ring` library, so hashing and encryption need no
native code in your app. There are two layers:

- **`crypto`**: `crypto.getRandomValues`, `crypto.randomUUID` and `crypto.subtle` (the part of Web Crypto a desktop app
  uses). Random bytes come from the system's secure source.
- **`@nile/crypto`**: short helpers for the common jobs (passwords, SHA-256, HMAC, encrypting a string under a password).

## Passwords

```js
import { hashPassword, verifyPassword, needsRehash } from "@nile/crypto";

const stored = await hashPassword("correct horse");        // "pbkdf2-sha256$600000$<salt>$<hash>": store this string
if (await verifyPassword(typedPassword, stored)) login();   // constant-time compare; false for a wrong or malformed hash
if (needsRehash(stored)) saveHash(await hashPassword(typedPassword));   // after a successful login, once the cost was raised
```

PBKDF2-HMAC-SHA256 with a random 16 byte salt and 600,000 iterations (the current OWASP guidance for PBKDF2-SHA256). The
string carries its own salt and cost, so raising the default later doesn't break old hashes. The work runs on a worker
thread (about 60 ms on a recent Mac, several hundred ms on slower machines), so the window stays responsive; don't call it in a
loop. `{ iterations }` changes the cost. **Argon2, scrypt and bcrypt are not available**: they need native code Nile doesn't
ship; PBKDF2 at this cost is still an accepted choice.

## Hashes, signatures, encryption

```js
import { sha256, digest, hmac, encryptText, decryptText, uuid, randomBytes } from "@nile/crypto";
await sha256("text");                       // hex; digest(data, "SHA-512") for the others (SHA-1 only for old formats)
await hmac("secret key", "message");        // hex HMAC-SHA256
const sealed = await encryptText("my password", "a note");     // AES-256-GCM, key from the password with PBKDF2
const note = await decryptText("my password", sealed);          // rejects (OperationError) on a wrong password or changed data
```

## The standard API

```js
const s = crypto.subtle;
await s.digest("SHA-256", bytes);                                            // SHA-1, SHA-256, SHA-384, SHA-512
const key = await s.importKey("raw", bytes, { name: "HMAC", hash: "SHA-256" }, false, ["sign", "verify"]);
const tag = await s.sign("HMAC", key, data);          await s.verify("HMAC", key, tag, data);
const base = await s.importKey("raw", encoder.encode(password), "PBKDF2", false, ["deriveBits", "deriveKey"]);
const bits = await s.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt, iterations: 600000 }, base, 256);
const aes = await s.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
const sealed = await s.encrypt({ name: "AES-GCM", iv: crypto.getRandomValues(new Uint8Array(12)) }, aes, data);
await s.decrypt({ name: "AES-GCM", iv, additionalData }, aes, sealed);       // ciphertext followed by the 16 byte tag
```

Supported: `digest`; HMAC (`generateKey`, `importKey`, `sign`, `verify`); PBKDF2 (`deriveBits`, `deriveKey`); AES-GCM, 128 or
256 bit keys, 12 byte IVs, 128 bit tags (`generateKey`, `importKey`, `exportKey`, `encrypt`, `decrypt`). Keys are `raw`
only. `getRandomValues` fills integer typed arrays up to 64 KiB. Heavy calls run on a worker thread and return Promises
like in a browser; errors have the standard names (`OperationError`, `NotSupportedError`, `InvalidAccessError`...).

## Not here

- RSA, ECDSA, ECDH, Ed25519, HKDF, AES-CBC/CTR and JWK key import/export. (Update signing uses Ed25519 inside the shell, not
  through this API.)
- Argon2/scrypt/bcrypt (see above) and secure key storage in the system keychain.
- A hardened environment: a script holding your key can read it. This is for apps' own data, not a security boundary
  against code you load.

Outputs are checked against the published SHA-2, HMAC, PBKDF2 and NIST AES-GCM test vectors in the test suite.
