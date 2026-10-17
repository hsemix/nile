# Native extensions

Nile loads **explicitly declared, trusted native packages**. They have the privileges
of the process: filesystem, networking, devices and native code execution. This is
not a sandbox or a way to run untrusted plugins. There is no native-library loader
exposed to JavaScript, automatic package search, npm resolution, or hot unloading.

The generic host is `nile-extensions`; `nile-extension-sdk` defines its public C ABI
and generates `include/nile_extension.h`. Neither depends on the engine, SQLite or
QuickJS. `nile-script` supplies the Promise/event adapter and module resolution.
The engine depends on this generic adapter, **not** on the SQLite package.

## Run the SQLite example

From the repository root, with Rust and a C compiler available:

```sh
python3 scripts/build-extension.py sqlite --dest examples/sqlite/extensions/sqlite
cargo run -p nile-shell -- examples/sqlite/index.html
```

Click **Insert and query**. It inserts Hamid using a bound parameter and displays
actual database rows. The UI timer continues while requests run. `customers.db`
is relative to `examples/sqlite/nile.json`, independent of subsequent changes to
process working directory. Repeated clicks persist additional rows.

The build script assembles an ordinary directory:

```text
sqlite/
  extension.json
  index.js
  native/
    macos-aarch64/libnile_ext_sqlite.dylib
```

A manifest contains only these fields (unknown fields are rejected in ABI v1):

```json
{
  "id": "sqlite",
  "version": "0.1.0",
  "abi": 1,
  "entry": "index.js",
  "native": {
    "macos-aarch64": "native/macos-aarch64/libnile_ext_sqlite.dylib"
  }
}
```

`id` is an ASCII alphanumeric/underscore operation namespace, unique within an
application configuration; `version` is a nonempty distribution version string,
not a dependency solver constraint. `abi` must equal the supported ABI version.
Entry/native paths must resolve to files within the package, including after
symlink resolution. Native code is trusted; containment is packaging validation,
not a security boundary. Required entry/library paths must exist.

The application opts in through `nile.json`:

```json
{"extensions":{"@sqlite":"./extensions/sqlite"}}
```

The nearest `nile.json` in the page directory or its ancestors is used. Configured
package directories resolve relative to that configuration, never a later CWD.
Only declared packages load, before classic or module startup scripts. All
packages initialize even if not imported. Import names must start with `@`, have
no slash, and cannot shadow `@nile`, `@sciter`, `@env`, `@sys`, `@debug`, `@storage`
or the internal `@native`. Each page/runtime has independent extension instances;
child pages discover configuration in the same way. Reload destroys the old
context before creating a fresh one, with new IDs.

Embedders can call `ScriptHost::configure_extensions(config)` before running scripts,
or `discover_extensions(page)`. They return detailed `Result` errors. `Page` uses
this automatically; an initialization error is reported to stderr and prevents
that page's scripts from starting, without terminating the host process. Existing
synchronous `host.*` APIs are unchanged.

**Packed applications are unsupported for native extensions in v1.** If the
configuration is in Nile's embedded filesystem, startup returns an explicit
unsupported error. An OS loader cannot load a library from Nile's archive. There
is no implicit extraction or external search fallback. Use an ordinary filesystem
application (a normal macOS app bundle with real package files is fine), or have a
custom host explicitly supply an external filesystem configuration. In particular,
`--pack` is not a supported distribution route for the SQLite example.

## Platforms and builds

Canonical names are `macos-{aarch64,x86_64}`, `windows-{aarch64,x86_64}-msvc`, and
`linux-{aarch64,x86_64}-{gnu,musl}`. There is no fallback between architectures,
C runtimes, operating systems or ABI versions. Other targets fail explicitly.
The package script uses an explicit Rust-target map and never relabels a library
for another platform. Standard suffixes are `.dylib`, `.dll`, `.so` respectively.

```sh
python3 scripts/build-extension.py sqlite --release --dest dist/sqlite
python3 scripts/build-extension.py notifications --release --dest dist/notifications
# Cross build only when the Rust target and native C toolchain are installed:
python3 scripts/build-extension.py sqlite --target x86_64-pc-windows-msvc --release --dest dist/sqlite
```

The script preserves manifest entries for previously assembled targets. A native
SQLite build bundles SQLite through `rusqlite`/`libsqlite3-sys`; it needs a C compiler.
Windows uses MSVC, macOS Xcode command line tools, and Linux a matching GNU/musl
compiler. In practice build each binary on its target CI runner. GNU Linux packages
must also target a sufficiently old glibc for their intended deployment systems;
the platform name alone is not a minimum-glibc guarantee. Musl shared-library
loading must be verified with a dynamically loadable host, not assumed to work in
a fully static executable. Initial CI exercises GNU Linux, macOS arm64 and Windows
x86_64, not every recognized cross-build target.

### Build independently from Nile

The extension manifests use only their own version/edition and the SDK path;
they inherit no engine workspace properties. Copy them and the SDK to a fresh
directory **outside** the workspace:

```sh
mkdir -p /tmp/nile-native/crates /tmp/nile-native/extensions
cp -R crates/nile-extension-sdk /tmp/nile-native/crates/
cp -R extensions/sqlite /tmp/nile-native/extensions/
cargo build --manifest-path /tmp/nile-native/extensions/sqlite/Cargo.toml --release
```

The reproducible automated check is `python3 scripts/check-extension-standalone.py`.
It builds release mode outside the workspace and checks the complete dependency
graph for engine/QuickJS dependencies.

This builds a shared library using only `nile-extension-sdk`, `serde_json` and
`rusqlite`. It does not build or link Nile's DOM, rendering, script, engine or
shell crates. The SDK may instead be distributed independently and referenced
by an ordinary registry or path dependency. SDK generation uses cbindgen. The
committed C header lets a C author compile without Rust or QuickJS headers:

```sh
cc -shared -fPIC -Icrates/nile-extension-sdk/include my_extension.c -o my_extension.so
```

Use `-dynamiclib` on macOS if preferred; on Windows use `cl /LD` and export
`nile_extension_init` with `__declspec(dllexport)`. The test fixture in
`crates/nile-extensions/tests/support/fixture.c` is a complete C consumer.

## JavaScript API

```js
import sqlite from '@sqlite';
const db = await sqlite.open('app.db', { readOnly: false, busyTimeoutMs: 1000 });
try {
  await db.execute('CREATE TABLE IF NOT EXISTS customers (id INTEGER PRIMARY KEY, name TEXT)');
  await db.execute('INSERT INTO customers (name) VALUES (?)', ['Hamid']);
  const rows = await db.query('SELECT id, name FROM customers WHERE name = ?', ['Hamid']);
  console.log(rows);
} finally {
  await db.close();
}
```

All four operations return Promises. `execute` returns `{changes, lastInsertRowId}`;
`query` returns an array of objects. Use SQL `AS` aliases for duplicate column names:
duplicates are rejected with `SQLITE_DUPLICATE_COLUMN`, avoiding silent data loss.
Column names such as `__proto__` are ordinary own properties. A query is limited
to 10,000 rows, 16 returned blob buffers and the host packet limit; use pagination.

Parameters are positional arrays and are always bound to prepared statements.
Exactly one SQL statement is accepted per operation. Nonempty statement tails
are rejected; trailing whitespace/comments are allowed. `execute` is for statements
without result rows; use `query` for `SELECT`/`RETURNING`. Empty paths, SQLite
URI paths and Windows drive-relative paths (such as `C:app.db`) are rejected. `:memory:` is supported. `readOnly` defaults to false;
`busyTimeoutMs` defaults to 1000 and has a maximum of 5000 milliseconds.

SQL NULL maps to JS `null`. Booleans bind as SQLite integers. Strings are UTF-8;
invalid SQLite text is rejected rather than silently replaced. Safe integer values
return as Number, other signed 64-bit integers as BigInt. Supply BigInt for input
integers outside JS's safe range; unsafe Number integers are rejected. Blobs use
`Uint8Array` or `ArrayBuffer` input and `Uint8Array` output, including empty blobs.
The wrapper encodes BigInt as `{"$i64":"decimal"}` and binary references as
`{"$blob":index}` in internal JSON; blob bytes travel in the ABI's separate buffer
array. These tags are wrapper protocol details, not SDK database concepts. Arbitrary
object parameters are rejected, so callers cannot accidentally inject these tags.

Errors reject with an Error carrying `.code` and `.message`. SQLite engine failures
use `SQLITE_<extended numeric SQLite code>`; validation failures use stable names
such as `SQLITE_CLOSED`, `SQLITE_HANDLE`, `SQLITE_INTEGER`, `SQLITE_PARAMETER`,
`SQLITE_MULTIPLE_STATEMENTS`, `SQLITE_DUPLICATE_COLUMN` and `SQLITE_RESULT_LIMIT`.

One extension worker owns all that context's SQLite connections. Requests run in
submission order, including open/queries/close. Already queued statements finish
before an explicit close. `db.close()` is idempotent and returns its original
Promise; subsequent statement calls reject with `SQLITE_CLOSED`. Raw repeated
native close/stale handle operations reject rather than finding another connection.
Explicit close plus context disposal manage resources; GC is not the cleanup API.
Closing a connection rolls back an unfinished transaction.

Use explicit `BEGIN`, `COMMIT`, `ROLLBACK` and `try/finally`. Transactions belong
to a connection; operations from unrelated callers sharing it can interleave between
awaits. Serialize the complete transaction in application code or use a dedicated
connection. There is deliberately no async transaction callback helper.

A statement Promise has `.cancel()`. It rejects the JS Promise and requests native
cancellation. Queued work checks cancellation before executing; active SQLite VM
work checks a progress handler every 1000 instructions. Cancellation is cooperative,
not a guarantee that a write did not occur. Closing the context likewise invalidates
Promises independently from stopping native work. Busy lock waits are bounded by
the timeout, while OS filesystem I/O, extension bugs and external native calls can
still delay shutdown; no forced thread termination is claimed.

## Write an extension

Export exactly one known initializer:

```c
NileStatus nile_extension_init(const NileExtensionApi *api, NileExtension *extension);
```

Validate `abi_version`, `struct_size` and required capability bits **before** reading
optional fields. Populate the output's ABI version, size, required capabilities,
opaque `user` value, and lifecycle hooks. Publish teardown hooks before starting
workers or registering operations, so partial initialization can be cleaned up.
The host provides a zero/default-initialized full-size output table. Disposal and
shutdown hooks are mandatory for successful initialization, even if they are no-ops.
A failed context initialization still receives disposal and shutdown.

Register namespaced operations with `api->register_operation(api->host, name,
dispatch, user)`. Names start with the manifest identity followed by `.`; duplicates
are rejected. Registration is allowed only while the initializer is running. There
is no database-specific registry. Future printing, MySQL, hardware and networking
packages can register their own names without engine changes.

The dispatch callback receives a borrowed `NileRequest`: owning context ID, unique
call ID, flags, operation, JSON, binary-buffer array and the absolute configuration
base path. Dispatch runs on the runtime/UI thread. Copy arguments you retain, enqueue
work or complete immediately, and return promptly. The host does **not** prescribe
one worker per extension: use a pool, an async runtime or a worker as appropriate.
SQLite's ordered worker is specific to that package. Do not perform blocking native
work in dispatch or lifecycle initialization.

`complete(host, context, call, reply)` may run inline or from a worker. It copies
JSON, error strings and bytes before returning. It never enters QuickJS. A nonzero
dispatch status produces a structured `DISPATCH_<status>` Promise rejection unless
a completion has already been accepted. Return `NILE_OK` after successfully scheduling
asynchronous work; every ordinary call requires one eventual terminal completion.
If completion returns `NILE_INVALID`/`NILE_OVERLOADED`, fix/reduce the reply and retry
with a bounded error. Closed or duplicate calls must not be retried.

Wrapper modules import the scoped bridge with `import native from '@native'`.
Only modules under a declared package root can resolve this import. The bridge
can invoke only operations registered by that particular package instance:

```js
import native from '@native';
const result = await native.call('printer.status', { printer: 'office' }, []);
// result = { value: decoded JSON, buffers: Uint8Array[] }
```

The bridge is internal implementation machinery, not a sandbox against application
scripts. It exposes no pointer, arbitrary library loading or undeclared extension.
Existing relative-module imports and JSX transformation continue through the same
loader. Public API classes/convenience behavior belong in the package's wrapper.

### Ongoing events

A wrapper calls `native.subscribe(operation, args, listener, buffers?)`, receiving
`{ready, unsubscribe}`. The request has `NILE_SUBSCRIBE`; its call ID is also its
subscription ID. Complete once to acknowledge (settling `ready`), then use `emit`
with that ID for ongoing events. Both callbacks copy payloads and return statuses.
The listener receives `{value,buffers}`. A rejected acknowledgment removes the JS
listener; an extension must not start ongoing production after rejecting its start.

Unsubscribe invalidates the host subscription and queued events, rejects an unsettled
`ready` with `CANCELLED`, and invokes the extension's cancel hook on the runtime thread.
The extension must stop/join that subscription's producers in its cancel hook, or
otherwise arrange that they stop before disposal reports success. No queued event
is delivered after unsubscribe or disposal. Repeated unsubscribe is harmless.

`extensions/notifications` is the independent non-database consumer: it acknowledges
a subscription, sends incrementing sequence events on a background thread, stops
on unsubscribe, and joins all remaining producers on context disposal. It tolerates
`NILE_OVERLOADED` by dropping that notification and continuing. Applications can
observe gaps in its sequence number.

## ABI ownership, threading and shutdown contract

All ABI structures have C layout and fixed-width integers; versioned API/request/
reply/extension tables include byte sizes. Callback pointers are C calling-convention
functions. IDs and opaque resource payloads are uint64 values, never engine objects,
Rust layouts or QuickJS values. The only pointers are callback pointers and explicit
borrowed slices/tables/output parameters. The current negotiated capability bits are
async completion, binary buffers, managed resources, events and cancellation requests.
Future incompatible changes require another ABI version; additive table fields use
size checks. Callers must provide at least the v1 table prefix.

Strings are length-delimited UTF-8, not NUL terminated; binary bytes are uninterpreted.
JSON must be valid UTF-8 JSON even for errors (`null` is sufficient). Request memory
is borrowed only until dispatch returns. Reply/event memory is extension-owned and
borrowed only until complete/emit returns; the host makes copies while accepting it.
The host never frees extension allocations. The extension never frees host allocations.
Allocate and release on the same side, including Windows CRT boundaries. Resource
payload storage belongs to the extension and is released through its own destructor.

The API table is immutable and remains at a stable address through shutdown. An
extension may copy it, as the consumers do; a copied callback plus host/context IDs
may report a late completion safely after context destruction, provided that native
code itself remains loaded. The original borrowed table pointer expires after a
successful shutdown. Late callbacks return `NILE_CLOSED`, using a token registry,
without dereferencing a destroyed context. The caller must still supply valid reply
pointers; hostile native code and undefined behavior are outside this contract.

Initialization, dispatch, context initialization/disposal, cancellation and shutdown
are called on the owning runtime thread. Completion, event emission and resource
operations are thread-safe. Resource destructors run on the releasing caller's thread,
or the runtime thread during teardown. Extensions synchronize their own payloads:
a successful resource lookup borrows the payload until the extension permits release;
it does not lock the payload or extend its lifetime across an unsynchronized close.
Never release a payload while another extension-owned task uses it. Host callbacks
may be called from dispatch/destructors; no lock is held across dispatch or destructors.
Do not reenter the runtime or recursively pump the event loop from a callback.

`resource_create` produces a process-unique monotonically allocated handle. Handles
are never recycled, so generation/stale validation is enforced without exposing a
reusable slot number. `resource_get`/`resource_release` validate extension **and**
context ownership; stale/cross-context handles return `NILE_INVALID`. Successful
release removes the handle before invoking its destructor exactly once; repeated
release is invalid. During disposal, lookup/new resources are closed. Disposal must
stop all users of payloads before returning; remaining payload destructors then run.
SQLite's worker owns the connections and closes its map during disposal, so its SDK
resource destructor needs no additional database cleanup.

The acceptance point of a terminal completion is the host queue lock after validation
and copying. It atomically changes pending to accepted; concurrent duplicates return
`NILE_DUPLICATE`. After delivery/cancellation the call no longer exists and returns
`NILE_CLOSED`. Event acceptance checks the active subscription at the same point.
Call IDs, context IDs, host scopes and resource handles are process-unique; old
completions cannot resolve a Promise in a reloaded/new runtime.

Limits per extension/context: 128 outstanding calls (including accepted responses not
yet drained), 128 active subscriptions, 128 resources, 128 queued events, 16 buffers
and 1 MiB combined JSON/error/binary bytes per request/reply/event. A call reserves
its terminal response slot. Overload returns `NILE_OVERLOADED`/rejects dispatch;
there is no unbounded fallback queue. Event overflow drops newest and returns
`NILE_OVERLOADED`; the producer chooses retry/drop semantics. JS listener work is
outside this native queue bound. Poll/drain regularly.

Only `ScriptHost::tick()` interacts with QuickJS to resolve/reject completions and
deliver events. `set_extension_waker` installs a thread-safe notification closure;
it must only signal the owning loop, never enter QuickJS inline. The shell uses
winit's event-loop proxy. A 30ms tick fallback handles custom hosts/headless use and
requests started before a waker is installed. Embedders can use the returned next-tick
duration, including through the existing C view tick API; they need no winit dependency.
Idle event sources retain a pending tick so notifications are not stranded.

Normal teardown is:

1. Stop accepting native work; invalidate pending calls/subscriptions/queued packets.
2. Reject/clear JS pending calls with `CONTEXT_CLOSED` and remove listeners.
3. Invoke context disposal. Stop/cancel worker work cooperatively and **join all
   workers/producers that can use callbacks or resources** before returning success.
4. Invoke remaining resource destructors on the owner thread.
5. Invoke extension shutdown to release instance state; no further native activity
   may remain after it reports success.
6. Remove registry entries, then release extension API state/library handles and JS runtime.

The host loads one instance per context, so disposal and application shutdown compose
without shared worker ownership. Libraries stay loaded throughout their instance
lifetime; no mid-context unloading or reloading API exists. If ABI output is incompatible
or disposal/shutdown reports failure, the host cannot prove code stopped: it retains
the library/API/state for process lifetime and reports that teardown failed, rather
than unloading possibly executing code. This exceptional quarantine is a deliberate
leak, not recovery or a claim of successful cleanup. Normal paths fully clean up.
A v1 extension must report failure rather than claim successful teardown with live workers.

Rust exports and **every callback** must catch panics, using SDK `guard` (including
void cancellation/resource callbacks). Both workspace profiles use `panic="unwind"`
so recoverable panics can become `NILE_PANIC`/structured errors. Independent Rust
builds must also use unwind. Abort-mode binaries, allocator/OOM aborts, UB and foreign
exceptions cannot be recovered by `catch_unwind`; no process isolation is promised.
SDK/loader errors return status/Result instead of calling exit/abort. A release build
must not silently weaken these guarantees by switching back to `panic="abort"`.

This ABI supports request/response and ongoing event services, not rendering integration,
real-time audio callbacks or zero-copy shared memory. Those require future explicit,
versioned contracts. A future MySQL extension can reuse the same operation/resource/
completion/event interfaces and implement its own connection worker/pool and JS classes.

## Verification

```sh
cargo test -p nile-extensions --test boundary
cargo test -p nile-script --test extensions
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
```

The integration tests build actual shared libraries in `target/extension-tests`,
compile/load a C consumer of the generated header, and import public wrappers through
the real module resolver. They assert exact values, ABI/manifest/symbol failures,
resource ownership/staleness, duplicate/late completion statuses, bounded event
queues, callback wakes, unsubscribe, timers during pending SQL, cancellation and
active-worker shutdown. They do not infer success from screenshots or file existence.
The workspace CI runs these on its existing Linux/macOS/Windows matrix. Cross-compiled
binary artifacts alone do not prove native execution on another platform.
