# Keeping secrets: `@nile/keychain`

Passwords, API keys and mail credentials belong in the operating system's credential store, not in a JSON file next to
the app. `@nile/keychain` puts them there:

```js
import { keychain } from "@nile/keychain";
keychain.service = "SoftSale";                         // your app's name; all its secrets live under it
await keychain.set("smtp-password", "s3cret");          // account name, secret
const pw = await keychain.get("smtp-password");         // the text, or null when nothing is stored
await keychain.remove("smtp-password");                 // true when something was deleted
await keychain.setBytes("db-key", bytes);               // binary secrets; getBytes() gives a Uint8Array or null
if (!(await keychain.available())) console.log(keychain.reason());
```

| Platform | Store |
|---|---|
| macOS | Keychain (a "generic password" item per service and account) |
| Windows | Credential Manager |
| Linux | the Secret Service over D-Bus (GNOME Keyring, KWallet, KeePassXC) |

Everything returns a Promise: the system may stop to ask the user to unlock the store, and the call runs on a worker thread
so the window keeps working. Calls reject with the system's message, for example when access is denied or no keyring
service is running (`available()` is the way to ask first). Pass `{ service }` to a call to override `keychain.service`;
with neither, the program's file name is used, which every app launched through the same `nile` binary shares, so set a name.

What this protects, and what it doesn't: the OS keeps the secret encrypted at rest and tied to the user's login. Other
programs running as the same user may be able to read it (macOS asks the user the first time another program tries), and
the script that stores a secret can of course read it back. It is the right place for a mail password or token; it is not
a defence against code you load yourself.

Combine it with `@nile/crypto` to encrypt larger data: keep a random key in the keychain and encrypt the data with
`crypto.subtle` AES-GCM.

The macOS and the Linux Secret Service paths were run only on macOS here (a round trip of text and bytes through the
real Keychain); the Windows and Linux stores are the well-used `keyring` crate and are compiled by CI but not exercised.
