# Built-in modules

[Reference index](README.md) · [Data tutorial](../guide/data-and-services.md)

Use modules from `<script type="module">` or imported JS/JSX files. Available
names are `@nile`, `@sciter`, `@env`, `@sys`, `@debug`, `@storage`; bare npm names
are not resolved. This page describes Nile's implementation, not every export
of Sciter's similarly named modules.

## `@nile` and `@sciter`

`@sciter` aliases `@nile`.

```js
import { $, $$, uuid, encode, decode } from '@nile';
const id = uuid();
const bytes = encode('Hello', 'utf-8');
$('#output').textContent = id + ': ' + decode(bytes);
```

| Export | Signature / result |
|---|---|
| `$`, `$$` | Selector → one element/null or array, against document |
| `on`, `once`, `off` | Document-level event helpers; delegate to document methods |
| `onGlobalEvent(name, handler)` | Registers a global window event listener |
| `offGlobalEvent` | Present but currently does not remove subscriptions; use global `removeEventListener` |
| `uuid()` | Random UUID-shaped string; uses Math.random, not a cryptographic generator |
| `encode(text, encoding='utf-8')` | ArrayBuffer; UTF-8 or latin1/ascii variants |
| `decode(bytes, encoding='utf-8')` | String from ArrayBuffer/typed bytes |
| `toBase64(bytes)`, `fromBase64(text)` | Base64 text / ArrayBuffer |
| `md5(value)`, `crc32(value)` | Byte/string input → lowercase MD5 hex / unsigned CRC32 number; these are not password-hashing APIs |
| `parseValue(text)` | JSON++ parser supporting the implementation's relaxed literals |
| `devicePixels(length, axis)` | Rounded physical pixel conversion; axis is accepted, percentage/layout semantics are limited |
| `VERSION`, `REVISION`, `QUICKJS_VERSION` | Compatibility strings, not authoritative release/build identification |
| `setModuleUrlResolver` | Accepted placeholder; does not implement bare-module resolution |
| `import`, `loadLibrary`, `compress`, `decompress` | Throw unsupported-operation errors |

Element/document helpers support namespaces, delegation and capture:

```js
const list = document.getElementById('list');
list.on('click.products', '[data-id]', function (event, matched) {
  console.log(matched.dataset.id);
});
list.once('^keydown', event => console.log(event.key));
list.off('.products');
```

Handler arguments are `(event, matched)` and `this` is the receiving element.
The `^` prefix requests capture. `off` can target an event, namespace or handler.
Source: [web.js](../../crates/nile-script/src/web.js),
[prelude.js](../../crates/nile-script/src/prelude.js).

## `@env`

| Export | Usage |
|---|---|
| `OS`, `PLATFORM`, `DEVICE` | Platform strings; DEVICE is `desktop` |
| `language()`, `country()` | Locale values derived from host environment |
| `userName()`, `machineName()`, `domainName()` | Host identity strings; may be empty |
| `arguments()` | Argument string array |
| `home(relative='')`, `homeURL(relative='')` | Application/runtime home path and file URL, as implemented by the host |
| `path(name, relative='')`, `pathURL(...)` | Named platform directories; see supported names below |
| `drives()` | Drive/root list |
| `variable(name)` | Read environment variable |
| `variable(name, value)` | Set variable; `null` removes it |
| `launch(path)` | Ask OS to open a file/URL/application |
| `exec(...args)` | Launch program through the environment bridge; not a shell parser |

These are native capabilities. Use trusted pages. For a process with readable
output and completion, use `@sys.spawn` rather than assuming `exec` returns a
process handle. Named paths: home, applications, root, desktop, appdata, downloads, documents,
music, videos and pictures. Exact platform fallback values are in
[envmod.rs](../../crates/nile-script/src/envmod.rs).

```js
import { path, pathURL, language } from '@env';
console.log(language(), path('documents'), pathURL('documents'));
```

## `@sys` files and directories

```js
import { fs } from '@sys';
const entries = await fs.readdir(__DIR__);
for (const entry of entries) {
  console.log(entry.name, entry.type === fs.UV_DIRENT_DIR ? 'folder' : 'file');
}
const buffer = await fs.readFile(__DIR__ + 'notes.txt');
console.log(new TextDecoder().decode(buffer));
```

Paths accept native strings and supported `file://`/`this://app/` URLs.
`__DIR__` is particularly useful for file-relative code. Packed assets are
read-only. Relative bare paths use the native path resolver; prefer explicit
base paths rather than assuming browser URL semantics.

| Operation | Arguments / result |
|---|---|
| `stat`, `statSync`, `sync.stat` | Path → metadata; follows symlinks |
| `lstat`, `lstatSync`, `sync.lstat` | Path → metadata without following symlink |
| `readdir`, `readdirSync`, `sync.readdir` | Path → `[{name,type}]`; directories use type 2, ordinary entries type 1 |
| `readFile`, `readFileSync`, `sync.readFile` | Path → ArrayBuffer; lowercase `readfile` aliases exist |
| `writeFile`, `writeFileSync`, `sync.writeFile` | Path, string/bytes → writes entire file (Nile extension) |
| `unlink`, `unlinkSync`, `sync.unlink` | Path → remove file |
| `mkdir`, `mkdirSync`, `sync.mkdir` | Path → create directory including missing parents |
| `rmdir`, `rmdirSync`, `sync.rmdir` | Path → remove empty directory |
| `rename`, `renameSync`, `sync.rename` | Old path, new path → rename/move |
| `copyfile`, `copyfileSync`, `sync.copyfile` | Source, destination → copy |
| `realpath`, `realpathSync`, `sync.realpath` | Path → canonical path |
| `splitpath(path)` | `[directory, basename]` |
| `open(path, flags='r')`, `openSync(...)` | Open-file object |
| `opendir(path)`, `opendirSync(path)` | Directory iterator object |

Promise forms of whole-file/metadata operations currently wrap **synchronous**
work; `await` does not move them to a worker. Avoid large reads on the UI thread.
`statSync/lstatSync` return null for missing files; `readdirSync` returns null for
missing/non-directory paths. Their promise forms reject instead. Other errors
throw/reject with an `error.code` such as `ENOENT`, `EACCES` or `EIO`.

Metadata fields include boolean `isFile`, `isDirectory`, `isSymbolicLink`,
`st_size` bytes, `st_mode`, and `st_mtime/st_atime/st_birthtime` seconds where
available. `st_ctime` is currently an alias of mtime, not inode-change time.
Packed metadata may omit timestamps. Directory enumeration reports directory
versus non-directory; it does not classify all OS file types accurately.

Constants: `S_IFMT`, `S_IFDIR`, `S_IFCHR`, `S_IFBLK`, `S_IFREG`, `S_IFLNK`,
`S_IFSOCK`, `S_IFIFO`; `UV_DIRENT_UNKNOWN/FILE/DIR/LINK/FIFO/SOCKET/CHAR/BLOCK`.
Prefer metadata booleans for portable classification.

### Open files and directory iterators

File objects expose `path`, `fileno()`, `readSync(length?, position?)`,
`writeSync(bytes, position?)`, `statSync()`, `closeSync()` and promise forms
`read/write/stat/close`. Reads return Uint8Array; explicit position seeks before
reading/writing. These promise forms also wrap synchronous work. Use byte arrays
for writes. Flags support `r`, `r+`, `w`, `w+`, `a`, `a+` and exclusive variants
with `x`, as interpreted by the native open implementation.

```js
import { fs } from '@sys';
const file = await fs.open(__DIR__ + 'notes.txt', 'r');
try { console.log(new TextDecoder().decode(await file.read(4096))); }
finally { await file.close(); }
```

`Dir.next()` returns a promise of an entry or `undefined` at the end;
`for await (const entry of dir)` is supported. `close()` ends iteration.
The directory's entries are enumerated up front, not lazily streamed from OS.
`sys.Pipe` (named pipes), `sys.TTY(fd)` and `fs.watch(path, cb)` exist. On Windows a pipe is a loopback socket
announced in a file in the temp folder, so it only connects Nile to Nile; `TTY` sizes and modes use `stty` on Unix
and fixed defaults elsewhere; `fs.watch` polls every 250 ms (events 1 = rename, 2 = change, 20,000 entries at most).

## `@sys` processes

```js
import { spawn } from '@sys';
// Supply an argv array so spaces and quoting do not need shell interpretation.
const process = spawn(['my-tool', '--version'], { stdout:'pipe', stderr:'inherit' });
const chunk = await process.stdout.read(4096); // Uint8Array, or undefined at EOF
const result = await process.wait();
console.log(chunk && new TextDecoder().decode(chunk), result);
process.stdout.close();
process.stdin.close();
```

`spawn(argv, options)` returns a Process synchronously. Options: `cwd`, `env`
(adds/overrides parent environment), `stdin/stdout/stderr` (`pipe` default,
`inherit`, `ignore`). A string argv is split on whitespace, with no shell quote
handling; arrays are preferred.

Process fields: `pid`, stream handles or null. `wait()` resolves `{exit_status, term_signal}`; `kill()` requests termination. Pipe streams expose `read(length?)`
→ Uint8Array/undefined at EOF, `write(bytes)` → byte count, `close()`, `fileno()`.
Blocking pipe/socket/process operations run on workers and need engine ticks to
settle. Drain stdout and stderr while the child runs to avoid a full pipe
blocking it; a single read is not a complete-output collector.

## `@sys` sockets and TLS

| Class/member | Contract |
|---|---|
| `new TCP()` | Allocate TCP handle |
| `connect({ip,port})` | Promise; hostnames resolved natively |
| `bind({ip,port})`, `listen()` | Configure listener |
| `accept()` | Promise of connected TCP |
| `read(length?)`, `write(bytes)` | Worker-backed promise of bytes/undefined or written count |
| `tls(hostname)` | Upgrade connected TCP to TLS; requires `nile-script/tls` feature |
| `shutdown()` | Shut down stream; promise wrapper |
| `close()`, `fileno()` | Release handle / get resource ID |
| `getsockname()`, `getpeername()` | `{ip,port,family}` |
| `new UDP()` | Allocate UDP handle |
| UDP `bind`, `connect` | Address setup; connect returns promise |
| UDP `send(bytes, address?)` | Promise of bytes sent |
| UDP `recv(length?)` | Promise of `{data:Uint8Array, flags, addr:{ip,port,family}}` |
| UDP `close/fileno/getsockname/getpeername` | Resource/address operations |

Default read/recv size is 65,536 bytes. TCP is a byte stream: one read need not
correspond to one message; frame your protocol. Close handles when finished.
TLS verifies certificates against the configured bundled roots. The `net`
engine feature enables script TLS as well as HTTP networking in this build.

## `@sys` environment and utilities

| Export | Result |
|---|---|
| `hrtime()` | BigInt nanoseconds based on runtime monotonic clock |
| `gettimeofday()` | `{sec,usec}` wall-clock time |
| `uname()` | Platform information object; some fields are fallback/empty values |
| `isatty(fd)` | Boolean for a native descriptor |
| `environ()`, `getenv(name)` | Environment object / string or undefined |
| `setenv(name,value)`, `unsetenv(name)` | Change process environment |
| `cwd()`, `chdir(path)` | Process working directory |
| `homedir()`, `tmpdir()`, `exepath()` | OS paths |
| `random(lengthOrTypedArray)` | Uint8Array allocation or fills target; **not cryptographic** |

These process-wide operations can affect other windows. Socket/process source:
[sys.rs](../../crates/nile-script/src/sys.rs). File/path source:
[envmod.rs](../../crates/nile-script/src/envmod.rs).

## `@storage`

```js
import { open } from '@storage';
const store = open('/absolute/path/settings.json');
store.root.theme = 'dark';
store.commit();
store.close();
```

`open(path, allowWrite=true)` is also available as `Storage.open`. Returns
`{root, commit(), close(), registerClass(cls), createIndex()}`. Root is a JSON
object graph; Dates use tags and registered class names can restore prototypes.
Register classes before first reading `root`, because loading/revival is lazy.
`close()` commits. `allowWrite=false` prevents writes. Circular object graphs
are unsupported; malformed JSON currently falls back to an empty root.
This is not SQLite or Sciter's storage format; it has no indexes, encryption,
transactions or concurrent-writer guarantees. `createIndex()` returns null.
Use [localStorage](web-api.md#storage) for string preferences and host services
for transactional business data.

## `@debug`

| Export | Behavior |
|---|---|
| `setUnhandledExceptionHandler(fn)` | Receive uncaught errors; null resets |
| `setConsoleOutputHandler(fn)` | `(subsystem,severity,message)`; true swallows output; severity 0/1/2 |
| `callStackAt(level)` | Partial parsed stack-frame information |
| `getUIDofElement(el)`, `getElementByUID(id)` | DOM arena ID ↔ wrapper |
| `objectKind(value)` | Simplified kind string |
| `sublimatedValue(value)` | Returns the input |
| Breakpoint/resource/highlight setters | Accepted placeholders; no inspector/breakpoints |
| `getStyleRulesOfElement`, `sublimatedValueElements`, `frameVariables`, `containerId` | Empty/default compatibility results |

```js
import { setUnhandledExceptionHandler } from '@debug';
setUnhandledExceptionHandler(error => console.error('Application error:', error.message));
```

## Application bootstrap

`run.js` can be the start file of an app. It runs in a hidden bootstrap window
and opens visible windows. `__DIR__`/`__FILE__` describe the current module/page
URL; packed apps use `this://app/`.

```js
const main = new Window({url:__DIR__+'index.html',width:1000,height:760});
main.on('close', () => application.quit(0));
application.run();
```

`application.quit(code=0)` requests exit. `start(gfx)` is accepted but does not
choose a renderer. `run()` returns immediately; the shell owns the event loop.
See [packaging](../guide/packaging.md) and [bootstrap example](../../examples/bootstrap/run.js).
