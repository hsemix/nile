# Web-style APIs

[Reference index](README.md) · [Data tutorial](../guide/data-and-services.md)

Nile supplies useful subsets of these APIs. Browser-only behavior such as page
navigation, origin isolation, cookies and a complete stream implementation is
not implied by the names.

## Fetch and request bodies

```js
async function loadProducts() {
  const response = await fetch('products.json');
  if (!response.ok) throw new Error('HTTP/file status ' + response.status);
  return response.json();
}
```

File-relative and `data:` requests work without networking. HTTP/HTTPS require
`nile-engine/net` (enabled by the shell; C library builds opt in with
`--features net`). Packed URLs use `this://app/`. Completion runs through workers
and the host's tick loop; keep ticking in custom hosts.

| API | Members and behavior |
|---|---|
| `fetch(input, init)` | URL/Request, method, headers, body, signal → Promise<Response> |
| `Headers(init)` | `get/set/append/delete/has`, `forEach`, entries/keys/values/iteration |
| `Request(input, init)` | URL, method, headers, signal, body readers and `clone()` |
| `Response(body, init)` | status/statusText/headers/url, ok, body readers, clone; `Response.json` and `Response.error` helpers |
| Body | `text()`, `json()`, `arrayBuffer()`, `bytes()`, `blob()`, `formData()`, `body`, `bodyUsed` |
| `AbortController` | `signal`, `abort(reason)` |
| `AbortSignal` | aborted/reason, abort event, `throwIfAborted`, static `abort(reason)`; no timeout/any helpers |

Bodies accept strings, bytes, Blob and FormData through the implemented
conversion layer. FormData uploads encode multipart content. HTTP errors resolve
a Response; transport errors reject. Abort rejects the JS request, but does not
guarantee cancellation of native worker I/O already running. Network responses
are buffered before script delivery; `Response.body` does not make HTTP transfer
incrementally streaming. Cookies, browser credentials and CORS are not provided.
Remote `<img>` rendering remains a separate unsupported feature.

```js
const controller = new AbortController();
const request = fetch('data.json', { signal:controller.signal });
request.then(r => r.json()).then(console.log).catch(console.error);
// controller.abort(); // reject if the request is still pending
```

## XMLHttpRequest

`open(method,url)`, `setRequestHeader`, `send(body)`, `abort`,
`getResponseHeader`, `getAllResponseHeaders` wrap fetch. Fields include
`readyState`, `status`, `statusText`, `responseURL`, `responseText`, `response`,
`responseType` and `timeout`. Supported response types are text/default,
`arraybuffer`, `json`. `withCredentials` is a compatibility field;
`overrideMimeType()` is a no-op.

Events include readystatechange, loadstart, load, error, abort, timeout and
loadend. Progress handlers can be assigned, but the buffered backend does not
supply incremental download/upload progress. There is no synchronous-XHR mode.

## Blob, File and FormData

| Object | API |
|---|---|
| `Blob(parts, {type})` | size/type, `slice(start,end,type)`, text/arrayBuffer/bytes/stream |
| `File(parts,name,{type,lastModified})` | Blob with name and lastModified; chooser files are exposed through input.files |
| `FormData(form?)` | append/set/get/getAll/has/delete, forEach and iteration |

```js
const body = new FormData();
body.append('name', 'Sample product');
body.append('attachment', new Blob(['example'], {type:'text/plain'}), 'example.txt');
// await fetch(yourEndpoint, {method:'POST', body});
```

These objects buffer data in memory. FormData can collect named controls from a
form, but does not implement native form submission. There is no object-URL
registry (`URL.createObjectURL`) or browser drag-and-drop pipeline.

## Streams

| Class | Supported subset |
|---|---|
| `ReadableStream(source)` | Controller enqueue/close/error, getReader, locked, cancel, pipeTo/pipeThrough, tee, async iteration, `from(iterable)` |
| Reader | read → `{value,done}`, cancel, releaseLock; `closed` is a placeholder pending promise |
| `WritableStream(sink)` | getWriter, locked, abort/close; writer write/close/abort/releaseLock |
| `TransformStream(transformer)` | readable/writable pair with transform/flush |
| `TextEncoderStream`, `TextDecoderStream` | Encoding transform wrappers |

```js
const stream = ReadableStream.from(['one', 'two']);
for await (const chunk of stream) console.log(chunk);
```

Queue-based streams do not implement byte/BYOB readers, full locking/cancellation
semantics or full backpressure. `tee()` can retain accumulated chunks. Encoding
streams are simple wrappers, so do not assume a decoder preserves incomplete
multibyte characters across every chunk boundary. For large transfers, control
chunking and memory use explicitly or use host-side streaming.

## WebSocket

```js
const socket = new WebSocket('ws://127.0.0.1:8080');
socket.addEventListener('open', () => socket.send('hello'));
socket.addEventListener('message', event => console.log(event.data));
socket.addEventListener('error', () => console.error('Socket failed'));
// socket.close(1000, 'Finished');
```

`ws://` uses `@sys` TCP; `wss://` requires `nile-script/tls`, enabled through the
engine's `net` feature. Constructor accepts URL and optional protocol string or
array. `send` supports text/binary; incoming data follows `binaryType` (`blob`
default or `arraybuffer`). Events: open/message/error/close. Members include
url, readyState, protocol, extensions, binaryType, send and close. State
constants CONNECTING/OPEN/CLOSING/CLOSED are supplied.

The implementation handles framing, fragmentation, ping/pong and close codes.
Compression/extensions are unsupported. `bufferedAmount` remains zero and is
not useful for flow control. This is a client API; serving sockets is an
application task. Custom hosts must tick for worker completions.

## Storage

| API | Persistence |
|---|---|
| `localStorage` | String key/value JSON backing file for file/packed pages |
| `sessionStorage` | String key/value data in this context's memory |
| Both | `length`, `key(index)`, getItem/setItem/removeItem/clear and property proxy |
| `Storage.open` / `@storage.open` | Separate JSON object store; see [modules](modules.md#storage) |

```js
const saved = localStorage.getItem('preferences');
let preferences = {theme:'light'};
try { if (saved !== null) preferences = JSON.parse(saved); } catch (_) {}
localStorage.setItem('preferences', JSON.stringify(preferences));
```

Local persistence follows the page path, not browser web-origin rules. Packed
apps use their executable identity. `NILE_STORAGE_DIR` sets the backing directory;
`off` disables automatic disk persistence. Hosts can set a backing file explicitly
with `Page::set_storage_file` / `nile_view_set_storage_file`. Writes are scheduled
and flushed by the runtime. Do not use these APIs for POS transaction guarantees
or coordination between processes; use a host database.

## Observers and media

| API | Main operations | Limits |
|---|---|---|
| MutationObserver(callback) | observe(target,options), disconnect, takeRecords | Subset of browser records/options; checked through runtime mutation dispatch |
| ResizeObserver(callback) | observe, unobserve, disconnect; entries expose contentRect and size arrays | Measures implemented boxes, no full browser box-mode semantics |
| IntersectionObserver(callback,options) | observe, unobserve, disconnect, takeRecords | root and threshold; rootMargin ignored |
| matchMedia(query) | matches/media, change listeners and legacy addListener/removeListener | Implemented query subset; not full CSS media compatibility |

```js
const viewport = document.getElementById('viewport');
const observer = new ResizeObserver(() => updateVisibleRows());
observer.observe(viewport);
```

Observers are checked after supported layout/scroll changes, through ticks and
sync operations. Do not change layout and synchronously re-measure every item
inside an observer callback. See [virtualization](performance.md).

## URLs, navigation and environment

`URL(input,base)` and `URLSearchParams(init)` provide implemented parsing and
query operations; URLSearchParams supports append/set/get/getAll/delete/has,
sort/forEach/iteration/string conversion. They are partial implementations.
`location` is read-only (href/protocol/host/path/query/hash): it tells a script
where its page lives. Nile has no navigation: no `history`, no `hashchange`, no
assigning `location`. Use `new Window`, frame load methods or your own routing
to show other content.

`navigator` exposes runtime identity/platform/locale placeholders rather than
all browser device APIs. Use `@env` for native platform information.
`devicePixelRatio`, viewport sizes and screen/window metrics come from the host.
`structuredClone` clones its supported data subset; it is not full transferable
structured-cloning support. `atob/btoa` encode binary strings;
`TextEncoder/TextDecoder` and `@nile.encode/decode` handle byte/text conversion.

Source: [web.js](../../crates/nile-script/src/web.js),
[fetch worker](../../crates/nile-engine/src/net.rs).
