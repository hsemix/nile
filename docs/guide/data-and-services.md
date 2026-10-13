# Data, storage and application services

[Guide index](README.md) · Related: [Embedding](embedding.md)

## Load data

```js
async function loadItems() {
  const response = await fetch('fetch.json');
  if (!response.ok) throw new Error('Load failed: ' + response.status);
  const data = await response.json();
  document.getElementById('title').textContent = data.title;
  return data.items;
}
loadItems().catch((error) => console.error(error.message));
```

Paths resolve against the page folder. File and `data:` requests work without
network support. HTTP/HTTPS requests require `nile-engine`'s `net` feature, enabled
in the development shell. For C hosts build `cargo build -p nile-capi --features net`.
Requests run on worker threads and complete through the engine's event/tick loop.
An embedder must keep servicing ticks for asynchronous completion.

`Headers`, `Request`, `Response`, `AbortController` and XMLHttpRequest are
available. Response has text/JSON access; this is not a complete browser network
stack. Blob, FormData, streams, WebSocket, cookies and CORS are not implemented.
Network support does not add remote `<img>` rendering or browser page navigation.

Try the bundled file-request example without an external server:

```sh
NILE_STORAGE_DIR=off cargo run -p nile-shell -- examples/fetch.html --png /tmp/nile-fetch.png --wait 500
```

The wait gives asynchronous requests time to finish. Check terminal logs as well
as the image; errors can still leave a successfully written PNG.

## Save a draft

Both storage APIs store strings. Use JSON for structured data:

```js
const editor = document.getElementById('editor');
const key = 'draft-v1';
const draft = localStorage.getItem(key);
if (draft !== null) editor.innerHTML = JSON.parse(draft).html;

function saveDraft() {
  localStorage.setItem(key, JSON.stringify({ html: editor.innerHTML }));
}
```

This assumes an app-owned draft for a `contenteditable` element. Restore once
when loading, before editing; repeatedly restoring `innerHTML` during typing
replaces nodes and affects selection/history. Render arbitrary external data as
text or validate its markup before using it as a document.

`sessionStorage` lives in memory. File-backed pages persist `localStorage` as JSON
in the user's data directory, with a separate backing file per page file. The
backing store follows the page path, not browser web-origin semantics. `NILE_STORAGE_DIR`
chooses a different directory; `NILE_STORAGE_DIR=off` disables automatic disk
persistence. Rust hosts can select a file with `Page::set_storage_file`; C hosts
use `nile_view_set_storage_file`. HTML-created hosts can explicitly choose storage
instead of relying on a file-page default.

The Workbench apps still use in-memory data; adding storage APIs to the engine
does not automatically make their demo save buttons persistent.

## Observe changes and adapt to the viewport

Available utilities include MutationObserver, ResizeObserver,
IntersectionObserver, `matchMedia`, URL/URLSearchParams, TextEncoder/TextDecoder,
`atob`/`btoa`, `structuredClone`, `navigator` and `devicePixelRatio`.

```js
const compact = matchMedia('(max-width: 900px)');
const updateLayout = () => document.body.classList.toggle('compact', compact.matches);
compact.addEventListener('change', updateLayout);
updateLayout();
```

Resize/intersection observations are checked after layout changes or scrolling.
Intersection `rootMargin` is ignored. `window.location` exposes the page's
`file://` URL, but only hash updates take effect; assigning another URL does not
navigate. `history.pushState`/`replaceState` exist without back/forward navigation.

## Built-in modules

```js
import { uuid } from '@nile';
import { OS, language } from '@env';
console.log(uuid(), OS, language());
```

`@sciter` is an alias for `@nile`. `@nile` supplies selection/event helpers,
encoding and hash utilities, JSON++ parsing, device-pixel conversion and global
events. `@env` exposes platform/environment information and operations such as
`launch` and `exec`. Those operations run programs; they are capabilities of the
runtime, not simulated UI actions. Bare npm package names, `@sys`, and `this://`
URLs are still unsupported. See [nile-modules.html](../../examples/nile-modules.html)
and [the module implementation](../../crates/nile-script/src/envmod.rs) for export
names and behavior rather than assuming complete Sciter compatibility.

## Use host functions for application-specific work

Storage and fetch cover common data needs. Native integrations such as database
access, privileged business operations or a custom save dialog still belong in
the host. `host.has(name)` and the JSON `host.name(...)` bridge let a page detect
and call registered services. See [embedding](embedding.md#connect-scripts-and-services).
