# Windows and embedding

[Guide index](README.md) · Next: [CLI reference](cli.md)

## Choose a host

| Approach | Use it for |
|---|---|
| `nile-shell` | Developing pages, opening examples, headless PNG renders |
| `nile-engine::Page` | Rust hosts that manage input and a pixel buffer |
| `nile-capi` / `libnile` | Hosts using C or an FFI binding |

The engine does not create a window. It supplies storage and file/data requests;
HTTP networking is available with its `net` feature. The shell enables networking
and supplies a winit window, software presentation, clipboard, IME and file chooser
integration. Other hosts supply their own platform integrations.

## Configure the development-shell window

Set attributes on the root `<html>` element:

```html
<html window-title="My workspace"
      window-width="900" window-height="640"
      window-min-width="480" window-min-height="320"
      window-alignment="5" window-resizable="true">
```

Widths/heights are logical pixels, or percentages of the desktop. Alignment uses
numeric-keypad positions: 5 centers, 7 is top-left, 9 is top-right. Other attributes
include `window-min-*`, `window-max-*`, `window-minimizable`, `window-maximizable`,
`window-icon`, and `window-state` (`shown`, `minimized`, `maximized`, `full-screen`,
`hidden`). `<title>` supplies a title when `window-title` is absent.

For frameless chrome:

```html
<html window-frame="solid" window-resizable="6px">
  <body>
    <div role="window-caption">
      My workspace
      <button role="window-minimize">Minimize</button>
      <button role="window-maximize">Maximize</button>
      <button role="window-close">Close</button>
    </div>
  </body>
</html>
```

The shell interprets these roles: the caption drags the window, double-click
toggles maximization, the buttons request native actions, and the edge margin
allows resizing. Style the elements yourself. See [window.html](../../examples/window.html).

`window-frame` accepts system frames (`default`/`standard`), `extended` (macOS
content under the title bar), `solid`/`solid-with-shadow`, and `transparent`.
`solid-with-shadow` currently has no native shadow. Desktop transparency depends
on the host surface; the software shell does not expose a surface alpha mode.

Scripts can set `document.title`, call `window.close()`, `moveTo`/`moveBy`,
`resizeTo`/`resizeBy`, and request/exit fullscreen. These enqueue host commands.
Headless runs have no native window, so they do not perform the native actions.
Multiple windows and a general native dialog API are not provided. File-input
chooser requests are supported through the shell or C-host callbacks.

## Embed from Rust

The engine's [Page API](../../crates/nile-engine/src/lib.rs) can load HTML or a file:

```rust
use std::path::PathBuf;
use nile_engine::Page;

let mut page = Page::load_html(
    "<body><h1>Hello from an embedded page</h1></body>",
    PathBuf::from("examples"),
    (640.0, 400.0),
);
let mut pixels = vec![0u8; 640 * 400 * 4];
page.paint(&mut pixels, 640, 400, 1.0);
```

Add `nile-engine` as a path dependency pointing to `crates/nile-engine` in your
checkout. Pixels are packed BGRA with premultiplied alpha. `Page::tick()` returns
whether a redraw is needed and the next scheduled duration. The view is
single-threaded. The public Rust API includes internal plumbing and may change;
the C API is the preferred embedding boundary for other hosts.

## Build and use the C library

```sh
cargo build -p nile-capi --release
```

This produces shared/static `libnile` libraries in `target/release` and regenerates
[nile.h](../../crates/nile-capi/include/nile.h). Library extensions depend on the
platform. A complete [C example](../../examples/c/render.c) loads a page, exposes
a host function, evaluates JavaScript, forwards input and writes a PPM image.
On macOS/Linux with a compatible C compiler:

```sh
cc examples/c/render.c -Icrates/nile-capi/include -Ltarget/release -lnile -Wl,-rpath,"$PWD/target/release" -o /tmp/nile-render
/tmp/nile-render examples/hello.html /tmp/nile-out.ppm
```

Windows needs the corresponding compiler/linker and library-path configuration;
the Unix `-rpath` command above does not apply.

### Host lifecycle

1. Create a view with `nile_view_create_file` or `nile_view_create_html`. Pass the
   logical viewport size; the HTML constructor also takes an asset base directory.
2. Forward pointer movement, primary-button presses/releases, wheel deltas and
   key events. Forward pointer coordinates in logical pixels, not physical pixels.
3. Call `nile_view_tick` after activity that can schedule work and when its returned
   `next_ms` elapses. `-1` means nothing is scheduled; `redraw` asks for painting.
4. Paint with `nile_view_render` into `width * height * 4` writable bytes, packed
   BGRA with premultiplied alpha. Width/height are physical pixels; `scale` is the
   physical-to-logical ratio. On resize, update the logical viewport.
5. Drain `nile_view_take_window_command` after input, script calls and ticks; apply
   close, minimize, maximize, movement, resizing and fullscreen in your host.
   Follow title/cursor changes too.
6. For several windows, also drain `nile_view_take_window_request` (JSON, free it with
   `nile_string_free`): `open` (`new Window()`/`modal()`: make a host window and a view with
   `nile_view_create_window`, passing the request's `id`, `opener`, `modal` and its `url`/`html`/
   `width`/`height`/`parameters` fields; `type`, `caption`, `alignment`... say how to show it),
   `post` (`nile_view_window_message` on the view whose `nile_view_window_id` is `to`), `close`
   (ask `nile_view_close_request` first, then tell the opener with `nile_view_window_closed`),
   `focus` and `quit`. Answer a window the user closed the same way (`close_request` with reason 0).
7. Destroy the view with `nile_view_destroy` when finished.

`nile_key` performs uncanceled default keyboard actions. `nile_text_input` inserts
committed text; avoid inserting the same character through both paths. Only the
primary mouse button is currently supported. For frameless dragging, call
`nile_view_caption_press` during the host's press handler and, if it returns true,
start a native drag instead of forwarding that press as ordinary page input.

Provide system clipboard callbacks with `nile_view_set_clipboard`; the default
clipboard is private memory. `nile_view_selected_text` returns the input/page
selection for a host Copy command. Update the window rectangle so script window
measurements reflect the actual host window.

### Input methods and file choosers

The shell forwards native IME composition through winit. An embedding host should
enable its input method when `nile_view_ime_wanted` returns 1, position its candidate
window using `nile_view_ime_cursor_area`, and forward preedit/commit/cancel through
`nile_view_ime_preedit`, `nile_view_ime_commit` and `nile_view_ime_cancel`. Candidate
geometry is in logical viewport coordinates; convert it to your platform's required
space. Check the header for cursor-range parameters. Do not also send committed
composition text through ordinary text input or it will be inserted twice.

For `<input type="file">`, drain `nile_view_take_file_request`, show your chooser,
and answer with `nile_view_choose_files`. The request includes `accept` and
`multiple`; the returned accept string must be freed with `nile_string_free`.
Consult the header for the selected-path encoding and cancellation convention.
The shell uses AppleScript, PowerShell or zenity/kdialog depending on the OS;
chooser availability therefore depends on installed platform tools.

HTTP fetch/XHR support requires `nile-capi --features net`; file/data requests
work without it. The tick loop services asynchronous completions. File-created
views persist localStorage by default; `NILE_STORAGE_DIR` or
`nile_view_set_storage_file` controls its backing location. See
[data and services](data-and-services.md) for storage semantics.

### Connect scripts and services

Register a callback with `nile_view_register_function(view, "name", callback, user)`.
Page scripts call it as `host.name(...)`. Arguments arrive as a JSON array; the
callback returns a JSON value as a NUL-terminated string, copied immediately by
Nile. Return `NULL` for JavaScript `undefined`. Return `"\"saved\""` for a JSON
string, not raw `"saved"`. Keep callbacks and their user data valid for the view's
lifetime. Register functions after creating the view and before triggering script
calls that require them; constructor-time scripts run before later registration.

Use `nile_view_eval` to evaluate JavaScript or `nile_view_call` to invoke a global
function with a JSON argument array. Module-local functions must be assigned to
`globalThis` if the host needs to call them by name. Call/tick or redraw afterwards
because scripts may mutate the DOM or schedule timers. Read console messages with
`nile_view_take_console`.

Returned `char *` strings belong to the caller: release them with
`nile_string_free`. Returned `const char *` strings are borrowed and should be
copied before the next call on that view. Use a view on one thread, and consult
the generated header for individual functions' contracts. The current interface
is early-stage; it has `nile_version()` but no dedicated ABI compatibility check.
