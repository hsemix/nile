# Native integration reference

[Reference index](README.md) · [Embedding tutorial](../guide/embedding.md)

## Rust host lifecycle

`nile_engine::Page` loads a file or markup with an asset base directory. The
application owns the window/event loop and presents the engine's BGRA pixels.

| Responsibility | Entry points |
|---|---|
| Create/reload | Page::load, load_html, load_window, html_window, reload |
| Resize/layout | Page::layout, View::set_viewport |
| Pointer | pointer_moved, pointer_left, press, release, context_menu |
| Wheel/keyboard | wheel, emit/default_key or the host's complete key forwarding pipeline |
| Timers/animation/async work | tick, animate; honor returned redraw/deadline |
| Rendering | paint; scale converts logical viewport to physical pixels |
| Services | set_dialogs, clipboard implementation, register_host_fn |
| Scripts/logs | eval, call, take_console |
| Storage | set_storage_file |
| Windows | load_window/html_window, view.window_requests, window_message/window_closed/close_request/window_activated |
| Accessibility | a11y_tree and a11y_action; map to platform nodes |
| Printing | paginate, render_page, write_pdf |

Use the [shell](../../crates/nile-shell/src/app.rs) as the integration example.
`Page` is single-threaded and uses Rc/RefCell internally. Do not hold a view/DOM
borrow while calling scripts. Public Rust plumbing can evolve; generate complete
crate documentation with `cargo doc --workspace --no-deps`.

## C ABI lifecycle and ownership

Build `cargo build -p nile-capi --release`. Add `--features net` for HTTP/HTTPS and
script TLS. Link the platform's shared library or statically link the archive;
the `.a` file is a link input, not the shipped executable's size.

1. Create a view from HTML/file. Check NULL/error results.
2. Send input in logical pixels, maintaining pointer position before buttons.
3. Tick to process timers, animation, fetch and native async completions.
4. Render BGRA premultiplied pixels at physical dimensions and device scale.
5. Drain window/file/menu requests and perform native operations in your host.
6. Notify child-window messages/closure/focus and update accessibility/IME.
7. Destroy the view and free returned strings/buffers according to the API.

Returned `char *` strings use `nile_string_free`; do not free them with the host's
allocator. Caller-owned render buffers must have sufficient size. Callback
lifetimes and user pointers belong to the host. Error/boolean/int conventions
vary by operation; the entries below state their contracts. Passing invalid
pointers is not made safe by the library's panic guards.

The authoritative declarations and enums are in
[nile.h](../../crates/nile-capi/include/nile.h); the entries below reproduce the
current exported-function contracts from [lib.rs](../../crates/nile-capi/src/lib.rs).
See [render.c](../../examples/c/render.c) for a runnable host.

## ABI version and struct sizes

`NILE_ABI_VERSION` (in `nile.h`) is the binary interface the header describes; `nile_abi_version()` is the one the loaded library
implements. **Compare them first** and refuse to run when they differ:

```c
if (nile_abi_version() != NILE_ABI_VERSION) { /* header and library do not match */ }
```

The version changes only for incompatible changes (a function's signature, a struct's layout). Version 2 is the current one;
version 1 was the unversioned API before it, whose structs had no size field.

Every struct that crosses the boundary (`NileTick`, `NileWindowCommand`, `NileWindowConfig`, `NileClipboard`, `NileDialogs`)
starts with `uint32_t struct_size`. Set it to `sizeof` your copy of the struct before you pass it, even when you only
read the result:

- **Structs the library fills** (`NileTick`, `NileWindowCommand`, `NileWindowConfig`): the library writes only the fields
  that fit in `struct_size` bytes, so a library newer than your header can add fields without overrunning your memory. A
  struct with `struct_size` 0 is refused (`nile_view_window_config` returns false; the others write nothing).
- **Structs you fill and pass by pointer** (`NileClipboard`, `NileDialogs`): callbacks that are past `struct_size` read as NULL,
  so a host built before `get_html` existed keeps working.

New fields are only ever added at the end. `examples/c/abi_check.c` is a smoke test of the whole surface that CI builds and runs.

## Types, callbacks and constants

| Type | Contract |
|---|---|
| `NileView` | Opaque page handle; one-thread ownership |
| `NileTick` | `struct_size`, `redraw` boolean and `next_ms` deadline; -1 means wait for input |
| `NileClipboard` | `struct_size`, then `get(user)` borrowed UTF-8 text or NULL; copied immediately. `set(text,user)` receives text. Keep callback/user lifetimes valid |
| `NileHostFn` | `(args_json,user)` receives JSON argument array; returned UTF-8 JSON is copied immediately; NULL means undefined |
| `NileWindowConfig` | `struct_size`, caption pointer (valid until next view call), width/height and limits, resizable/margin/minimizable/maximizable, state/alignment/x/y/frame/frameless |
| `NileWindowCommand` | `struct_size`, `kind`, `a`, `b`; NONE when queue has no simple command |
| `NileCursor` | Default, pointer, text, crosshair, move, not-allowed, wait, progress, help, grab/grabbing, horizontal/vertical/diagonal resize or hidden |
| `NileDialogs` | `struct_size`, then blocking callbacks `open_files`, `save_file`, `pick_folder`, `message` and `user`; a NULL callback makes that dialog unavailable |
| `NileHotkeyState` | REGISTERED=0, REFUSED=1, PRESSED=2 (`nile_view_hotkey_event`) |
| `NileA11yAction` | CLICK=0, FOCUS=1, SET_VALUE=2, INCREMENT=3, DECREMENT=4, SCROLL_INTO_VIEW=5 |

Modifier bits combine with OR: SHIFT=1, CONTROL=2, ALT=4, META=8.
Window config `state`: 0 shown, 1 minimized, 2 maximized, 3 fullscreen, 4 hidden;
`frame`: 0 system, 1 extended, 2 solid, 3 transparent. Config alignment is 0
unspecified or keypad 1–9.

Simple command kinds: NONE=0, CLOSE=1, MINIMIZE=2, MAXIMIZE=3, RESTORE=4,
TOGGLE_MAXIMIZE=5, FULLSCREEN=6 (`a` boolean), MOVE_TO=7 (`a`,`b` screen pixels),
RESIZE_TO=8 (`a`,`b` logical content size). Extended open/post/close/focus/quit
requests use the JSON window-request API instead. These C numeric encodings
are not the JavaScript Window constants' encodings.

## Exported function contracts

### nile_version

The engine's version, e.g. "0.1.0".

```c
const char *nile_version(void);
```

### nile_abi_version

The binary interface version this library implements (see `NILE_ABI_VERSION` in the header).

```c
uint32_t nile_abi_version(void);
```

### nile_view_create_html

Create a view from HTML text. Relative paths (`<img src>`, `<script src>`, `url()`) resolve
against `base_dir` (NULL for the current directory). `width` x `height` is the viewport in
logical pixels. Returns NULL on bad arguments. Free it with [`nile_view_destroy`].

**Safety**
`html` and `base_dir` must be NULL or valid NUL-terminated strings.

```c
struct NileView *nile_view_create_html(const char *html,
                                       const char *base_dir,
                                       float width,
                                       float height);
```

### nile_view_create_file

Create a view from an HTML file. Relative paths resolve against the file's folder. A missing
file gives a page that says so. Returns NULL on bad arguments.

**Safety**
`path` must be a valid NUL-terminated string.

```c
struct NileView *nile_view_create_file(const char *path, float width, float height);
```

### nile_view_destroy

Destroy a view. Passing NULL is fine.

**Safety**
`view` must come from a `nile_view_create_*` call and not be used afterwards.

```c
void nile_view_destroy(struct NileView *view);
```

### nile_view_reload

Reload the page from its file or HTML text, keeping the size, scroll position and clipboard.

**Safety**
`view` must be a live view.

```c
void nile_view_reload(struct NileView *view);
```

### nile_string_free

Free a string returned as `char *` by this library. Passing NULL is fine.

**Safety**
`s` must be a pointer this library returned, not freed before.

```c
void nile_string_free(char *s);
```

### nile_view_resize

Set the viewport size in logical pixels (call when your window resizes).

**Safety**
`view` must be a live view.

```c
void nile_view_resize(struct NileView *view, float width, float height);
```

### nile_view_content_height

The page's full height in logical pixels (for your own scroll bar), or 0 on error.

**Safety**
`view` must be a live view.

```c
float nile_view_content_height(struct NileView *view);
```

### nile_view_render

Paint the page into `pixels`: `width` x `height` physical pixels, 4 bytes each in BGRA order
with premultiplied alpha (the usual native bitmap layout on little-endian machines), rows
packed with no padding. `scale` is the device pixel ratio (the logical viewport is
`width / scale` by `height / scale`). `len` is the buffer size in bytes and must be at least
`width * height * 4`. Returns false if the arguments are wrong.

**Safety**
`pixels` must point to `len` writable bytes.

```c
bool nile_view_render(struct NileView *view,
                      uint8_t *pixels,
                      size_t len,
                      uint32_t width,
                      uint32_t height,
                      float scale);
```

### nile_pointer_move

The pointer moved to (`x`, `y`) in logical pixels from the view's top-left. Returns true if
the page changed (hover styles, selection drag, ...) and needs drawing.

**Safety**
`view` must be a live view.

```c
bool nile_pointer_move(struct NileView *view, float x, float y);
```

### nile_pointer_leave

The pointer left the view.

**Safety**
`view` must be a live view.

```c
bool nile_pointer_leave(struct NileView *view);
```

### nile_pointer_button

A mouse button went down (`pressed` true) or up at the last pointer position. Only the
primary button (0) is used. `modifiers` is a mix of the `NILE_MOD_*` bits (Shift-click
extends selections, Cmd/Ctrl-click toggles list items). Returns true if the page changed.

**Safety**
`view` must be a live view.

```c
bool nile_pointer_button(struct NileView *view, int32_t button, bool pressed, uint8_t modifiers);
```

### nile_wheel

Scroll by a wheel or trackpad delta in logical pixels at the pointer. Positive `dy` scrolls
the content down the screen (wheel up). Always returns true.

**Safety**
`view` must be a live view.

```c
bool nile_wheel(struct NileView *view, float dx, float dy);
```

### nile_key

A key went down or up. `key` is the DOM `KeyboardEvent.key` value: "a", "Enter", "ArrowLeft",
"Backspace", " ", ... `modifiers` is a mix of the `NILE_MOD_*` bits. A key-down that scripts
don't cancel also does the default thing (typing, caret movement, Tab focus, shortcuts such as
Cmd/Ctrl+C). Returns true if the page changed.

**Safety**
`view` must be a live view and `key` a valid string.

```c
bool nile_key(struct NileView *view, const char *key, uint8_t modifiers, bool pressed);
```

### nile_text_input

Type text (for example what an input method committed) into the focused input, one character
at a time. Returns true if the page changed.

**Safety**
`view` must be a live view and `utf8` a valid string.

```c
bool nile_text_input(struct NileView *view, const char *utf8);
```

### nile_view_tick

Run due timers, animations and the caret blink. Call it when the time from the last
result's `next_ms` has passed, and after any call that might start something.

**Safety**
`view` must be a live view; `out` NULL or writable.

```c
void nile_view_tick(struct NileView *view, struct NileTick *out);
```

### nile_view_cursor

The pointer shape to show at the last pointer position.

**Safety**
`view` must be a live view.

```c
enum NileCursor nile_view_cursor(struct NileView *view);
```

### nile_view_selected_text

The text selected on the page or in the focused input (for a Copy menu item), as a new string
to free with [`nile_string_free`]; empty if nothing is selected.

**Safety**
`view` must be a live view.

```c
char *nile_view_selected_text(struct NileView *view);
```

### nile_view_set_clipboard

Install clipboard callbacks (the default is a private in-memory clipboard).

**Safety**
`view` must be a live view; the callbacks and `user` must stay valid for its lifetime.

```c
void nile_view_set_clipboard(struct NileView *view, const struct NileClipboard *clipboard);
```

### nile_view_accessibility_tree

The page's accessibility tree as JSON (`{"root", "focus", "nodes": [{"id", "role", "name",
"bounds": [x, y, w, h], "children": [ids], ...}]}`, window coordinates in logical pixels), as a
new string to free with [`nile_string_free`]. Build the platform's accessibility objects from it
(UI Automation, NSAccessibility, AT-SPI) and call it again after the page changes.

**Safety**
`view` must be a live view.

```c
char *nile_view_accessibility_tree(struct NileView *view);
```

### nile_view_accessibility_action

Carry out a request from assistive technology on node `id` of the tree last returned. `value` is
used by `SetValue` (NULL otherwise). Returns true if the page did something (draw again).

**Safety**
`view` must be a live view; `value` NULL or a NUL-terminated string.

```c
bool nile_view_accessibility_action(struct NileView *view,
                                    uint64_t id,
                                    enum NileA11yAction action,
                                    const char *value);
```

### nile_view_menu

The page's application menu (`Window.this.menu = element`) as JSON, or NULL when it has none:
`[{"id", "label", "accel"?, "enabled", "checked", "role"?, "items"?}, {"separator": true}, ...]`.
Build the platform's menu bar from it and call [`nile_view_menu_click`] with an item's `id` when it
is chosen. Call again after the page changes (`checked` and `enabled` follow its attributes).
Free the string with [`nile_string_free`].

**Safety**
`view` must be a live view.

```c
char *nile_view_menu(struct NileView *view);
```

### nile_view_menu_click

A menu item was chosen: fires its `click` (or does a clipboard/editing role). Returns NULL, or for a
window role the host must carry out (`quit`, `close`, `minimize`, `zoom`, `about`, ...) a string
to free with [`nile_string_free`].

**Safety**
`view` must be a live view.

```c
char *nile_view_menu_click(struct NileView *view,
                           uint64_t id);
```

### nile_view_menu_accelerator

Run the menu item whose shortcut is `key` (a DOM key such as `"s"` or `"F5"`) with the same modifier
bits as `nile_view_key`, for hosts whose menu bar doesn't handle shortcuts. Returns whether one ran.

**Safety**
`view` must be a live view and `key` a valid string.

```c
bool nile_view_menu_accelerator(struct NileView *view,
                                const char *key,
                                uint8_t modifiers);
```

### nile_view_write_pdf

Cut the document into pages (`@page` size and margins, `break-*`) and write them to a PDF at
`path`. Returns the page count, or -1 on failure. The view's own layout is redone on the next draw.

**Safety**
`view` must be a live view and `path` a valid string.

```c
int32_t nile_view_write_pdf(struct NileView *view,
                            const char *path);
```

### nile_view_title

The page's current title (`document.title`); valid until the next call on the view.

**Safety**
`view` must be a live view.

```c
const char *nile_view_title(struct NileView *view);
```

### nile_view_window_config

Read the window settings from the page's root element. `desktop_width`/`desktop_height` (the
screen's logical size) resolve percentage sizes and alignment. Returns false on error.

**Safety**
`view` must be a live view and `out` writable.

```c
bool nile_view_window_config(struct NileView *view,
                             float desktop_width,
                             float desktop_height,
                             struct NileWindowConfig *out);
```

### nile_view_take_window_command

Take the next pending window request (close, minimize, move, ...). Returns false, with `out`
set to `NILE_WINDOW_COMMAND_KIND_NONE`, when there are none; call it in a loop after every
input call and tick.

**Safety**
`view` must be a live view; `out` NULL or writable.

```c
bool nile_view_take_window_command(struct NileView *view, struct NileWindowCommand *out);
```

### nile_view_window_id

The id this view's window has (what other windows address it by, and `Window.this.id`).

**Safety**
`view` must be a live view.

```c
uint32_t nile_view_window_id(struct NileView *view);
```

### nile_view_take_window_request

The next thing this window's scripts asked of the host that involves other windows or the
program, as JSON, or NULL when there is none. Free it with [`nile_string_free`]. Kinds:

- `{"kind":"open","id":3,"opener":1,"modal":false,"fields":{"url":"a.html","width":"400",...}}`:
`new Window({...})` / `modal({...})`. Create a window and call [`nile_view_create_window`] with
the `id`, `opener`, `modal` and the `url`, `html`, `width`, `height` and `parameters` fields; the
other fields (`type`, `caption`, `x`, `y`, `alignment`, `state`, ...) say how to show it.
- `{"kind":"post","from":1,"to":3,"json":{...}}`: deliver with [`nile_view_window_message`] to
the view whose [`nile_view_window_id`] is `to`.
- `{"kind":"close","id":3,"value":null}`: close that window (ask [`nile_view_close_request`] first;
tell its opener with [`nile_view_window_closed`] afterwards).
- `{"kind":"focus","id":3}`: bring it to the front.
- `{"kind":"quit","code":0}`: `application.quit()`.

**Safety**
`view` must be a live view.

```c
char *nile_view_take_window_request(struct NileView *view);
```

### nile_view_create_window

Create the view for a window the page asked to open (an `"open"` request from
[`nile_view_take_window_request`]). `url` is a file or `this://` URL resolved against `base_dir`
(normally the opener's folder), or NULL when `html` has the markup; both NULL gives an empty page.
`parameters_json` is the `parameters` field (NULL for none). Returns NULL on bad arguments.

**Safety**
The string arguments must be NULL or valid NUL-terminated strings.

```c
struct NileView *nile_view_create_window(const char *url,
                                         const char *html,
                                         const char *base_dir,
                                         float width,
                                         float height,
                                         uint32_t id,
                                         uint32_t opener,
                                         bool modal,
                                         const char *parameters_json);
```

### nile_view_window_message

A message another window posted (`Window.post`): fires `message` with `data` (`json_data`) and
the sender's id as `source`. Returns whether the page changed.

**Safety**
`view` must be a live view; `json_data` a valid string.

```c
bool nile_view_window_message(struct NileView *view, uint32_t from, const char *json_data);
```

### nile_view_window_closed

A window this one opened has closed; resolves its `Window.closed` / `modal()` promise with
`value_json` (JSON text, `"null"` for none). Returns whether the page changed.

**Safety**
`view` must be a live view; `value_json` a valid string.

```c
bool nile_view_window_closed(struct NileView *view, uint32_t id, const char *value_json);
```

### nile_view_close_request

The user (or a script) wants this window closed: fires `closerequest`. Returns whether it may close
(false if a listener called `preventDefault()`). `reason` is 0 for the title bar's button, 1 for a script.

**Safety**
`view` must be a live view.

```c
bool nile_view_close_request(struct NileView *view,
                             uint32_t reason);
```

### nile_view_set_window_rect

Tell scripts where the window is (`window.screenX`, `outerWidth`, ...): top-left in screen
pixels, outer size, and whether it is full screen.

**Safety**
`view` must be a live view.

```c
void nile_view_set_window_rect(struct NileView *view,
                               float x,
                               float y,
                               float width,
                               float height,
                               bool fullscreen);
```

### nile_view_caption_press

For a frameless window: call after the pointer is pressed over the view. Returns true if the
press landed on a `role="window-caption"` element and you should start dragging the window
(a quick second press asks to toggle maximize instead, via the window request queue); in
that case don't pass the press on with [`nile_pointer_button`].

**Safety**
`view` must be a live view.

```c
bool nile_view_caption_press(struct NileView *view);
```

### nile_view_eval

Evaluate JavaScript in the page. On success returns 0 and stores the result in `*out`:
strings as they are, `undefined` as an empty string, anything else as JSON. On a script error
returns 1 and stores the message. `out` may be NULL. Free `*out` with [`nile_string_free`].
After it returns, call [`nile_view_tick`] or draw again, as scripts may have changed the page.

**Safety**
`view` must be a live view, `code` a valid string and `out` NULL or writable.

```c
int32_t nile_view_eval(struct NileView *view, const char *code, char **out);
```

### nile_view_call

Call a JavaScript function by name (`"save"` or dotted, `"app.save"`) with arguments given as
a JSON array (`"[1, \"two\"]"`; NULL means no arguments). Returns 0 and the JSON result in
`*out`, or 1 and an error message. Free `*out` with [`nile_string_free`].

**Safety**
`view` must be a live view, `name` a valid string, `args_json` NULL or valid, `out` NULL or writable.

```c
int32_t nile_view_call(struct NileView *view,
                       const char *name,
                       const char *args_json,
                       char **out);
```

### nile_view_register_function

Expose `callback` to scripts as `host.<name>(...)`. Registering a name again replaces it.

**Safety**
`view` must be a live view and `name` a valid string; `callback` and `user` must stay valid
for the life of the view.

```c
bool nile_view_register_function(struct NileView *view,
                                 const char *name,
                                 NileHostFn callback,
                                 void *user);
```

### nile_view_set_storage_file

Keep the page's `localStorage` in the file at `path` (created if missing). Pages created from
a file already persist into the user's data directory; set `NILE_STORAGE_DIR` (`off` to
disable) to move that. Returns 0 on success.

**Safety**
`view` must be a live view and `path` a valid string.

```c
int32_t nile_view_set_storage_file(struct NileView *view, const char *path);
```

### nile_view_take_file_request

A click on `<input type=file>` asks the host for a file chooser. If one is waiting, returns 1,
sets `*multiple` (0 or 1) and `*accept` (the `accept` attribute, free it with
[`nile_string_free`]; either may be NULL), and remembers the input; answer with
[`nile_view_choose_files`]. Returns 0 if nothing is waiting.

**Safety**
`view` must be a live view; `multiple` and `accept` NULL or writable.

```c
int32_t nile_view_take_file_request(struct NileView *view, int32_t *multiple, char **accept);
```

### nile_view_choose_files

Answer the request from [`nile_view_take_file_request`] with the chosen files, one path per
line (`"a.txt\nb.txt"`); an empty string clears the input. Returns 0 on success, 1 if no
request was taken.

**Safety**
`view` must be a live view and `paths` a valid string.

```c
int32_t nile_view_choose_files(struct NileView *view, const char *paths);
```

### nile_view_ime_wanted

Whether the focused element takes text, so the host should enable its input method (IME).
Returns 1 or 0.

**Safety**
`view` must be a live view.

```c
int32_t nile_view_ime_wanted(struct NileView *view);
```

### nile_view_ime_cursor_area

Where the input method's candidate window should go: the caret, in logical pixels of the
view. Returns 1 and fills the four outputs (any may be NULL), or 0 if there is no caret.

**Safety**
`view` must be a live view; the outputs NULL or writable.

```c
int32_t nile_view_ime_cursor_area(struct NileView *view, float *x, float *y, float *w, float *h);
```

### nile_view_ime_preedit

The input method is composing `utf8` text. `cursor_start`/`cursor_end` are the composition's
own caret as byte offsets into the text, or both -1 for none. An empty string ends the
composition. Returns 1 if the page changed.

**Safety**
`view` must be a live view and `utf8` a valid string.

```c
int32_t nile_view_ime_preedit(struct NileView *view,
                              const char *utf8,
                              int32_t cursor_start,
                              int32_t cursor_end);
```

### nile_view_ime_commit

The input method finished composing: insert `utf8` text where the composition was.
Returns 1 if the page changed.

**Safety**
`view` must be a live view and `utf8` a valid string.

```c
int32_t nile_view_ime_commit(struct NileView *view, const char *utf8);
```

### nile_view_ime_cancel

Abandon the composition (the input method was switched off or lost focus).

**Safety**
`view` must be a live view.

```c
void nile_view_ime_cancel(struct NileView *view);
```

### nile_view_take_console

The `console.log`/`warn`/`error` lines written since the last call, separated by newlines
(empty if none), as a new string to free with [`nile_string_free`].

**Safety**
`view` must be a live view.

```c
char *nile_view_take_console(struct NileView *view);
```

### nile_view_set_dialogs

Install native dialogs for scripts and `<input type=file>` (without them scripts get `null`
and file inputs use `nile_view_take_file_request`). Kept across reloads.

**Safety**
`view` must be a live view; the callbacks and `user` must stay valid for its lifetime.

```c
void nile_view_set_dialogs(struct NileView *view, const struct NileDialogs *dialogs);
```

### nile_view_set_system_theme

Tell Nile the system's colour theme: `dark` for `prefers-color-scheme: dark` (and the dark
look of pages that say `color-scheme: light dark`), `accent_rgb` the system accent colour as
`0xRRGGBB` (negative = the platform's stock accent). It is a per-thread setting shared by every
view of the thread; call it before creating views and again when the OS changes. Restyles this
view (call it on the others too) and returns true if the theme changed.

**Safety**
`view` must be a live view.

```c
bool nile_view_set_system_theme(struct NileView *view, bool dark, int32_t accent_rgb);
```

### nile_view_drop_files

Files were dropped on the window at the pointer's last position (tell the page where with
`nile_pointer_move` first). `paths` are UTF-8 paths separated by newlines. A file input under
the pointer takes them; elsewhere the target gets a `drop` event. Returns true if the page changed.

**Safety**
`view` must be a live view and `paths` a valid string.

```c
bool nile_view_drop_files(struct NileView *view,
                          const char *paths);
```

### nile_view_window_activated

The window became active (`active` true) or inactive: fires `activate` on `Window.this`.
Returns true if the page changed.

**Safety**
`view` must be a live view.

```c
bool nile_view_window_activated(struct NileView *view, bool active);
```

### nile_view_portals

Popups that want an OS window of their own (`popup({window: "popup"})`), as JSON
`[{"id":N,"x":..,"y":..,"w":..,"h":..}]` with the border box in this view's logical coordinates (it
may lie outside the view). Free the string with `nile_string_free`. For each: make a frameless,
always-on-top window at that rectangle, draw it with `nile_view_render_portal`, and send its
pointer, wheel and key input to this same view with `x`/`y` offset by the rectangle's origin.

**Safety**
`view` must be a live view.

```c
char *nile_view_portals(struct NileView *view);
```

### nile_view_render_portal

Draw one popup from `nile_view_portals` into its window's buffer (same format as
`nile_view_render`): `width` x `height` physical pixels at `scale`, the popup's top-left
corner at the buffer's. Returns false if `id` isn't a current popup or the arguments are wrong.

**Safety**
`pixels` must point to `len` writable bytes.

```c
bool nile_view_render_portal(struct NileView *view,
                             size_t id,
                             uint8_t *pixels,
                             size_t len,
                             uint32_t width,
                             uint32_t height,
                             float scale);
```

### nile_view_tray

The tray icon the page asked for (`Window.this.tray = {...}`) as JSON, or NULL when it has none:
`{"tooltip": "...", "template": bool, "icon": "<base64 of the icon file's bytes (PNG)>", "menu": [...]}`, where `menu` has the
shape of `nile_view_menu` (choose an item with `nile_view_menu_click`). Show the icon in the notification area / menu
bar, report clicks with `nile_view_tray_event`, and call again after the page changes. Free the string with
`nile_string_free`.

**Safety**
`view` must be a live view.

```c
char *nile_view_tray(struct NileView *view);
```

### nile_view_tray_event

The tray icon was clicked (`"trayclick"`) or double-clicked (`"traydoubleclick"`): fires that event on the document.
Returns true if the page changed.

**Safety**
`view` must be a live view and `kind` a valid string.

```c
bool nile_view_tray_event(struct NileView *view,
                          const char *kind);
```

### nile_view_badge

The dock / taskbar badge text (`application.badge`), or NULL when none is set. Free it with `nile_string_free`.

**Safety**
`view` must be a live view.

```c
char *nile_view_badge(struct NileView *view);
```

### nile_view_progress

The dock / taskbar progress (`application.progress`): returns 0 for none, 1 for indeterminate, 2 for a value, which is
then written to `value` (0.0 to 1.0; `value` may be NULL).

**Safety**
`view` must be a live view; `value` NULL or writable.

```c
int32_t nile_view_progress(struct NileView *view,
                           float *value);
```

### nile_view_hotkeys

The global hotkeys the page registered (`application.registerHotkey`), as JSON
`[{"id": 1, "accelerator": "CmdOrCtrl+Shift+K"}]`. Register each with the system, report the outcome with
`nile_view_hotkey_event` (`Registered` or `Refused`) and later each press (`Pressed`). Accelerators are modifiers
(`Ctrl`, `Shift`, `Alt`/`Option`, `Cmd`/`Super`, `CmdOrCtrl`) and a key joined by `+`. Call again after the page changes; ids
that vanished from the list were released. Free the string with `nile_string_free`.

**Safety**
`view` must be a live view.

```c
char *nile_view_hotkeys(struct NileView *view);
```

### nile_view_hotkey_event

Report a global hotkey's registration result or a press, by the `id` from `nile_view_hotkeys`. Returns true if the
page changed.

**Safety**
`view` must be a live view.

```c
bool nile_view_hotkey_event(struct NileView *view,
                            uint32_t id,
                            enum NileHotkeyState state);
```

### nile_view_set_screens

Tell scripts about the monitors (`Window.screens`, `Window.screenBox`, `Window.this.screen`): `screens_json` is
`[{"name": "...", "x": 0, "y": 0, "width": 2880, "height": 1800, "scale": 2.0, "primary": true}]` (position and size in
physical pixels, `scale` device pixels per CSS pixel) and `current` the index of the monitor the window is on. Call it when
the window is created and whenever the monitors or the window's monitor change: a change after the first call fires
`resolutionchange` on the window. Returns true if the information changed.

**Safety**
`view` must be a live view and `screens_json` a valid string.

```c
bool nile_view_set_screens(struct NileView *view,
                           const char *screens_json,
                           int32_t current);
```

### nile_view_application_event

The system delivered an application event: `kind` is `"second-instance"` (another copy was started: `data_json` is
`{"arguments": [...], "cwd": "..."}`), `"open-url"` (`{"url": "..."}`) or `"open-file"` (`{"path": "..."}`). Scripts get an
event of that name, carrying the fields of `data_json`, on `document` and `window`. `kind` may contain only lower-case
letters and `-`, and `data_json` must be a JSON object (or NULL), or nothing happens. Returns true if the page changed.

**Safety**
`view` must be a live view; `kind` and `data_json` valid strings.

```c
bool nile_view_application_event(struct NileView *view,
                                 const char *kind,
                                 const char *data_json);
```

### nile_view_maximize_button_rect

The rectangle of the page's first `role="window-maximize"` element in physical pixels (`x, y, width, height` into
`rect`), for hosts that give a frameless window Windows 11 Snap Layouts. Returns false when there is none.

**Safety**
`view` must be a live view; `rect` must point to four writable floats.

```c
bool nile_view_maximize_button_rect(struct NileView *view,
                                    float *rect);
```

### nile_view_window_corner_radius

The corner radius in logical pixels the root element asks for (its `border-radius`): clip a frameless window to it.

**Safety**
`view` must be a live view.

```c
float nile_view_window_corner_radius(struct NileView *view);
```
