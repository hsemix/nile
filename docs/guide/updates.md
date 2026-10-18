# Updating a packed app

An update replaces the app's **content** (its HTML, CSS, scripts, images and bundled native
extensions), not the `nile` runtime. That keeps updates small, makes them work the same on every
platform, and avoids replacing a running (and on macOS, signed) program. A new runtime still ships
as a normal download or installer; an update can say which runtime it needs.

Updates are signed. The trust starts at the install: the app's own `nile.json` names the public key
every update must be signed with, and a downloaded update that isn't signed with it is refused.

## One-time setup

```sh
nile --keygen mykey            # writes mykey.key (keep it secret, out of the repository) and prints the public key
```

Put the public key and the place updates are published in the app folder's `nile.json`:

```json
{
  "id": "com.example.notes",
  "version": "1.0.0",
  "update": {
    "publicKey": "LMQ1z8tDaXSnsMnUqqgU616f77YU5QdARznY93cJCAo=",
    "url": "https://example.com/notes/update.json"
  }
}
```

then pack and ship the app as usual (`nile --pack notes ...`).

## Publishing an update

Raise `"version"` in `nile.json`, then:

```sh
nile --release notes --key mykey.key --notes "Faster search" -o dist
```

This writes `dist/notes-1.1.0.nilepack` (the app's archive) and `dist/update.json`, the signed
manifest. Upload both next to each other (the manifest's `url` is relative to the manifest, or
give an absolute one with `--url`). Options: `--version` (default: from `nile.json`),
`--notes`, `--min-runtime 0.2.0` (refuse to install on older runtimes).

```json
{ "version": "1.1.0", "url": "notes-1.1.0.nilepack", "sha256": "…", "signature": "…",
  "size": 482113, "notes": "Faster search" }
```

## In the app

```js
application.version;                         // "1.0.0" (from nile.json)

const update = await application.checkForUpdates();   // fetches update.url; null when up to date
if (update) {
  await application.downloadUpdate(update);  // downloads, checks hash + signature, stages it
  // the new version starts the next time the app starts, or now:
  application.restartForUpdate();
}
application.updateReady;                     // true when a verified newer update is staged
```

`checkForUpdates(url)` takes a manifest URL instead of the one in `nile.json`. Both calls use `fetch`,
so `https://` (and `file://` for testing) work. `restartForUpdate()` starts a new copy of the program,
which waits for the old one to quit (it needs `app-single-instance` only to avoid two windows; the
wait is automatic) and then the old copy ends.

`examples/updater` is a small app with a "Check for updates" button.

## How it is checked

- The download's SHA-256 must equal the manifest's, and the manifest's Ed25519 signature
  (over `nile-update-v1`, the version and the hash) must verify with the app's key.
- The version must be newer than the running one.
- The staged update is kept in the user's data folder (`~/Library/Application Support/nile/updates/<id>`,
  `%LOCALAPPDATA%\nile\updates\<id>`, `~/.local/share/nile/updates/<id>`), and is checked **again** at every
  start before it is mounted in place of the app's own files. A damaged file or one for an app that has
  since been reinstalled with a newer version is ignored.
- An update's own `nile.json` may carry a new `publicKey`; it then signs the updates after it (key rotation).
- Native extension libraries inside an update go through the same hash checks as ones packed into the app.

## Limits

- Only content is updated; the runtime (and the executable's icon and version information) is not.
- No delta downloads and no rollback to an earlier version yet; no background checking (call
  `checkForUpdates()` when you want one).
- Downloads are held in memory and passed to Rust as base64, which is fine for app content but not for
  hundreds of megabytes.
