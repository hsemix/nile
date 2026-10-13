# nile

An open-source, embeddable HTML/CSS/JS UI engine in Rust, in the spirit of
[Sciter](https://sciter.com): its own renderer, no webview, small enough to
ship inside a desktop app.

*"nile" is a working title.*

**New to Nile?** Start with the [usage guide](docs/guide/README.md) and
[your first app](docs/guide/getting-started.md). The guide covers layout, scripting,
controls, selection, custom menus, native windows, embedding and the snapshot CLI.

![milestone 1](docs/milestone-1.png)

## Status: milestone 1

The full pipeline runs end to end:

```
HTML ──html5ever──▶ DOM ──cascade──▶ computed styles ──Taffy + Parley──▶ boxes ──Skia──▶ pixels
                     │
                     └──▶ <script> ──▶ QuickJS
```

| Crate | Job | Built on |
|---|---|---|
| `nile-dom` | Arena DOM, HTML parsing | `html5ever` |
| `nile-style` | CSS parsing, cascade, inheritance, UA sheet | `cssparser` |
| `nile-text` | Shaping, line breaking, font fallback | `parley` |
| `nile-layout` | Block + flex layout, text measurement | `taffy` |
| `nile-render` | Backgrounds, borders, radii, glyphs | `skia-safe` |
| `nile-script` | JS runtime, DOM bridge, events, modules and JSX | `rquickjs` |
| `nile-engine` | The page without a window: input, forms, scrolling, painting into a buffer | |
| `nile-shell` | The `nile` binary: native window around the engine, headless PNG | `winit`, `softbuffer` |
| `nile-capi` | C library (`libnile`) and `nile.h` for embedding | `cbindgen` |

## Embedding (C API)

`cargo build -p nile-capi --release` produces `libnile` (a shared and a static library) and
`crates/nile-capi/include/nile.h`. The host owns the window and the event loop: create a view
(`nile_view_create_html` / `nile_view_create_file`), forward pointer, wheel and key events
(`nile_pointer_move`, `nile_pointer_button`, `nile_wheel`, `nile_key`, `nile_text_input`), call
`nile_view_tick` when its `next_ms` elapses, and draw with `nile_view_render` into your own
BGRA buffer. The page talks back through `nile_view_take_window_command` (close, minimize,
maximize, move, resize, fullscreen), `nile_view_title`, `nile_view_cursor`,
`nile_view_window_config` (the `window-*` attributes) and `nile_view_caption_press` (frameless
dragging). Input methods (IME): the host enables its IME while `nile_view_ime_wanted` is 1, puts the candidate window at `nile_view_ime_cursor_area`, and forwards `nile_view_ime_preedit` / `nile_view_ime_commit` / `nile_view_ime_cancel`; the `nile` binary does this through winit's `Ime` events. The composition shows underlined at the caret in text inputs and in `contenteditable` hosts (`--compose TEXT` renders one headless; `examples/ime.html`). Scripts and host call each other: `nile_view_eval`, `nile_view_call`,
`nile_view_register_function` (scripts see it as `host.name(...)`, JSON in and out) and
`nile_view_take_console`. The clipboard is yours through `nile_view_set_clipboard`.
Single-threaded; no panic crosses the boundary. See `examples/c/render.c` for a complete program
that renders a page to an image:

```sh
cargo build -p nile-capi --release
cc examples/c/render.c -Icrates/nile-capi/include -Ltarget/release -lnile -Wl,-rpath,$PWD/target/release -o render
./render examples/hello.html out.ppm
```

Not in the C API yet: multiple mouse buttons, touch, GPU surfaces (it paints
into CPU memory), drag and drop, and a stable ABI version check beyond `nile_version()`.

## Running it

Requires recent stable Rust. On macOS you also need the Xcode command line
tools (`xcode-select --install`). The first build downloads prebuilt Skia
binaries from GitHub (macOS arm64/x86_64, Windows, Linux).

```sh
cargo run -p nile-shell -- examples/hello.html      # window: F5 or Ctrl/Cmd+R reloads, wheel scrolls
cargo run -p nile-shell -- examples/hello.html --png out.png --scale 2
cargo run -p nile-shell -- examples/interact.html --png out.png --hover 40,40   # simulate the pointer
cargo run -p nile-shell -- examples/scripting.html --png out.png --click 60,80 --wait 400   # click, run timers
cargo run -p nile-shell -- examples/measure.html --png out.png                              # layout queries from JS
cargo run -p nile-shell -- examples/todo/index.html --png out.png --click 60,128          # modules + JSX + Sciter helpers
cargo run -p nile-shell -- examples/forms.html --png out.png --click 60,66 --type "Ada{Backspace}x"   # form controls
cargo run -p nile-shell -- examples/scrolling.html --png out.png --wheel 100,100,-60                    # element scrolling
cargo run -p nile-shell -- examples/forms.html --png out.png --click 60,66 --type "Ada{Home}{Shift+End}{Cmd+c}"   # selection
cargo test --workspace
```

## Word-style ribbon demo

```sh
cargo run -p nile-shell -- examples/word-ribbon.html
cargo run -p nile-shell -- examples/word-ribbon.html --png docs/word-ribbon.png --width 1280 --height 900 --scale 2
```

A ribbon around a live `contenteditable` document. Font, style and alignment
commands act on the selection; lists, clipboard commands, undo/redo, search, word
count and table insertion work too. Toolbar buttons preserve the editor's caret.
Save/AutoSave, sharing and printing remain demo controls; the page-break command
inserts a horizontal rule. See the [rich-text editing guide](docs/guide/rich-text-editing.md)
for usage, supported commands and current limits.

## Desktop application showcase

For an editable Excel-style grid with formulas, two worksheets, cell formatting
and local saving, run:

```sh
cargo run -p nile-shell -- examples/excel/index.html
```

See the [spreadsheet guide](examples/excel/README.md) for supported calculations,
keyboard controls and limits.

```sh
cargo run -p nile-shell -- examples/workbench/index.html
```

Nine interactive interfaces with shared navigation: settings, dashboard, kanban,
mail, code studio, inventory, calendar, media library and presentations. They use
existing HTML/CSS/JS APIs and in-memory sample data. See
[the showcase guide](examples/workbench/README.md) for workflows and validation.

![Nine Nile application interfaces](docs/workbench/overview.png)

## What's supported today

- **Selectors** (Servo's `selectors` crate): type, `.class`, `#id`, attributes (`[a]`, `[a^=b]`, ...), the
  descendant / `>` / `+` / `~` combinators, `:first-child`, `:last-child`, `:nth-child()`, `:not()`, `:is()`,
  `:root`, `:empty`, and `:hover` / `:active` / `:focus` (driven by pointer and focus events).
  Rules with unsupported selectors (`:visited`, `::first-line`, ...) are dropped.
- **Overflow and scrolling:** `overflow` / `overflow-x` / `overflow-y` (`hidden` clips to the padding box; `auto` and
  `scroll` also scroll with the wheel, with scroll chaining to the page, and draw overlay scrollbars). Hit testing,
  hover and `getBoundingClientRect` follow scrolled content; `scrollTop`/`scrollLeft`/`scrollWidth`/`scrollHeight`,
  `scrollTo`/`scrollBy` and `scroll` events work on elements (see `examples/scrolling.html`). Inputs and textareas
  scroll to keep the caret visible.
- **Properties:** `display` (block, inline, inline-block, flex, grid, none), `box-sizing`, `width/height` + min/max, `margin`, `padding`,
  `border`, `border-radius`, `background(-color)`, `color`, `font-size/weight/family`,
  `line-height`, `text-align`, `white-space` (`nowrap`), `font-style`, `text-decoration` (underline, line-through), `flex`, `flex-direction/wrap/grow/shrink/basis`,
  `justify-content`, `align-items`, `gap`.
- **JavaScript DOM** (QuickJS): `document`, `Element`/`Node`/`Text`, `querySelector(All)`, `matches`, `closest`,
  `classList`, `style`, `dataset`, attributes, `innerHTML`/`textContent`, `createElement`, tree edits,
  `addEventListener` with capture/bubble/once, `on*` properties and attributes, `click`/`mouse*`/`key*`/`focus`
  events, `DOMContentLoaded`/`load`, timers, `requestAnimationFrame`, promises, `<script src>` (see
  `examples/scripting.html`). Layout queries: `getBoundingClientRect`, `offset*`/`client*`, `getComputedStyle`
  (common properties), `innerWidth/Height`, `scrollTo`/`scrollY`/`scrollIntoView` (see `examples/measure.html`).
- **Web platform:** `fetch` (`Headers`, `Request`, `Response`, `AbortController`) and `XMLHttpRequest` load files next
  to the page and `data:` URLs, and http(s) when the `net` feature of `nile-engine` is on (the `nile` binary has it;
  `nile-capi` needs `--features net`); requests run on worker threads, so the page never blocks
  (`examples/fetch.html`). Also `MutationObserver`, `URL`/`URLSearchParams`, `sessionStorage` (in memory) and `localStorage` (saved as JSON in the user's data
  directory, one file per page file; `NILE_STORAGE_DIR` moves it or `off` disables it, `Page::set_storage_file` / `nile_view_set_storage_file` choose a file), `matchMedia`, `ResizeObserver` and `IntersectionObserver` (checked after each layout change or scroll; `rootMargin` is ignored), `window.location` (the page's `file://` URL; only `hash` changes take effect), `history.pushState`/`replaceState` (no back/forward), `navigator`, `devicePixelRatio`, `structuredClone`, `atob`/`btoa` and `TextEncoder`/`TextDecoder`.
- **Built-in modules:** `import … from "@nile"` (`"@sciter"` is an alias, so Sciter apps keep working) (`on`/`off`/`once`, `$`/`$$`, `uuid`, `encode`/`decode`, `toBase64`/`fromBase64`, `md5`, `crc32`, `parseValue` (JSON++), `devicePixels`, `onGlobalEvent` as window events) and `"@env"` (`OS`, `PLATFORM`, `DEVICE`, `language`, `country`, `userName`, `machineName`, `domainName`, `arguments`, `launch`, `home`/`homeURL`, `path`/`pathURL`, `drives`, `variable`, `exec`), verified against docs.sciter.com/docs/JS.runtime; `examples/nile-modules.html`. `launch` and `exec` run programs, so don't load untrusted pages into an embedder that doesn't want that.
- **Modules and JSX:** `<script type="module">` and `import` of relative files (`./x.js`, `../y.jsx`). JSX in any
  script compiles like Sciter's: `<h1 id="a">Hi</h1>` becomes `JSX("h1", {id: "a"}, ["Hi"])`, which returns a
  vnode array `[tag, attrs, children]`. `append`/`prepend`/`content` render vnodes (strings, nodes and arrays
  too), `patch(vnode, onlyChildren)` reconciles existing DOM, and function components are called as
  `Component(props, kids, parent)`. Replace `globalThis.JSX` to change what JSX means. See `examples/todo/`.
- **Sciter-style helpers** (checked against docs.sciter.com): `$()`/`$$()` (also `el.$`, `el.$$`),
  `el.on("click.ns ^keydown", [selector], handler)` where `handler(evt, matched)` runs with `this` = the element
  it is attached to, `el.once()`, `el.off(name | ".ns" | handler)`, `el.state` flags (`hover`/`active`/`focus`
  from interaction; `checked`, `disabled`, `readonly`, `expanded`, `collapsed`, `current`, `selected`, `invalid`,
  `busy` are runtime flags matched by the same-named `:pseudo-classes`), `elementIndex`, `nodeIndex`,
  `insertAfter`.
- **Form controls:** `<input>` (text, password, placeholder, `value`), `<textarea>` (wrapping, newlines, Up/Down
  by line), checkboxes and radio groups (including `<label>` clicks), `<button>`, disabled states, a blinking
  caret with click-to-place that scrolls into view, typing, Backspace/Delete/arrows/Home/End, Tab and Shift+Tab
  focus order (`tabindex` respected), and `input`/`change`/`focus`/`blur` events. Undo/redo (Ctrl/Cmd+Z, Shift+Ctrl/Cmd+Z, Ctrl+Y; typing and Backspace runs group into one step), word-wise
  movement and deletion (Alt or Ctrl+arrows/Backspace/Delete, Cmd+arrows/Backspace on macOS), and keyboard scrolling
  outside text inputs (arrows, Page Up/Down, Space, Home/End on the focused or hovered scroller, then the page).
  **Window** (`examples/window.html`): the root `<html>` element configures the native window as in Sciter
  (docs.sciter.com/docs/HTML/html-window): `window-title`, `window-width`/`-height` (px or % of the desktop),
  `window-min-*`/`-max-*`, `window-resizable` (`true`, `false` or an edge margin like `6px`),
  `window-minimizable`/`-maximizable`, `window-state` (`shown`, `minimized`, `maximized`, `full-screen`, `hidden`),
  `window-alignment` (1-9 on the numeric keypad's layout, 5 = centre), `window-icon`, and `window-frame`
  (`default`/`standard`, `extended` (macOS: content under the title bar), `solid`/`solid-with-shadow` (no system frame),
  `transparent`). With no system frame, elements with `role="window-caption"` drag the window (double-click
  maximizes) and `role="window-minimize"`/`window-maximize`/`window-close` are the buttons; the edges resize it.
  Scripts get `document.title` (set and get), `window.close()`, `moveTo/moveBy/resizeTo/resizeBy`,
  `screenX/Y`, `outerWidth/Height` and `element.requestFullscreen()`/`document.exitFullscreen()`.
  **Rich-text editing** (`examples/editor.html`, and `examples/word-ribbon.html`, a working word processor): any element with `contenteditable` is an editor. Click, drag, double- and triple-click to select; type; Enter splits paragraphs and list items (Shift+Enter inserts `<br>`); Backspace/Delete (with Alt for words); arrows, Home/End, Cmd/Ctrl+arrows (Cmd = line edges on a Mac, Ctrl = words), with Shift to extend; Cmd/Ctrl+A/Z/Y/B/I/U/C/X/V. `document.execCommand` runs `bold`, `italic`, `underline`, `strikeThrough`, `subscript`, `superscript`, `foreColor`, `backColor`/`hiliteColor`, `fontName`, `fontSize`, `removeFormat`, `justify*`, `insertUnorderedList`/`insertOrderedList`, `indent`/`outdent`, `formatBlock`, `insertText`, `insertParagraph`, `insertLineBreak`, `insertHTML`, `insertImage`, `insertHorizontalRule`, `createLink`/`unlink`, `delete`, `selectAll`, `undo`/`redo`, `copy`/`cut`/`paste`; `queryCommandState`/`Value`/`Enabled`/`Supported` drive toolbars. Nile adds `fontSizePx` and `blockStyle` (`"prop: value; ..."` on the selected blocks). With a caret, a format command applies to the next typed text. `window.getSelection()` has `anchorNode`/`focusNode`/offsets, `getRangeAt`, `addRange`, `collapse`, `extend`, `setBaseAndExtent`, `selectAllChildren`, `toString`; `document.createRange()` gives a `Range` (`setStart`/`setEnd`, `selectNodeContents`, `toString`, `deleteContents`, `insertNode`). Editors fire `beforeinput` (cancelable), `input` and `selectionchange`. A `mousedown` handler that calls `preventDefault()` keeps focus and selection where they are, so toolbar buttons don't steal the caret.
  Page text selection (off by default, like a desktop UI: opt in with `user-select: text`/`all`, a `selectable` attribute, or Sciter's `behavior: htmlarea`; `text-selection-background-color` sets the highlight): drag, double-click (word), triple-click (run), Shift+click, Ctrl/Cmd+A and Ctrl/Cmd+C, with `window.getSelection().toString()`. Text selection in inputs and
  textareas: Shift+arrows/Home/End, drag, Shift+click, double-click (word), triple-click (all), Ctrl/Cmd+A, and
  copy/cut/paste through the system clipboard; Tab selects an input's contents. In JS: `selectionStart`,
  `selectionEnd`, `setSelectionRange()`, `select()`. `<select>` with `<option>`s: a drop-down that opens on click,
  Space/Enter or Alt+Down, with mouse hover, arrows/Home/End, type-ahead, Escape, disabled options, and `input`/
  `change` events; sized to its longest option; a long list scrolls (wheel, arrows) and opens upward when there is
  more room above. `<optgroup label>` headings (bold, unselectable, options indented; `examples/optgroup.html`) work in drop-downs and list boxes. `<select multiple>` and `size > 1` are list boxes drawn inline: click, drag to select a range, Cmd/Ctrl-click to toggle,
  Shift-click to extend, arrows/Page/Home/End (Shift extends), Cmd/Ctrl+A, type-ahead, wheel (see
  `examples/listbox.html`). In JS: `value`, `selectedIndex`, `selectedOptions`, `options`, `option.selected`/
  `text`/`label`/`value`, `length`. In JS: `value`, `checked`, `disabled`, `type`, `name`,
  `placeholder`, `focus()`, `blur()`, `document.activeElement` (see `examples/forms.html`).
- **Pointer interaction:** hit testing (down to the inline element under the pointer), `:hover`, `:active` and
  click-to-focus on links and form controls, restyled live (see `examples/interact.html`).
- **Inline content:** mixed text, `<br>`, inline padding/border/background/radius and `inline-block` (with
  baseline) wrap as one flow (see `examples/inline.html`, `examples/selectors.html`).
- **Cascade and values:** `!important`, custom properties with `var(--x, fallback)` (inherited, can reference each
  other), `@media` (width/height features incl. range syntax, `orientation`, `screen`, `and`/`not`/commas; re-evaluated
  when the window resizes). `calc()` (including `100% - 20px`, resolved in layout), `min()`, `max()`, `clamp()` with absolute units.
  **Units:** `px`, `em`, `rem`, `%`, `pt`, `vw`, `vh`, `vmin`, `vmax`, `cm`, `mm`, `in`, `pc`. **Colors:** hex (3/4/6/8
  digits), `rgb()/rgba()` (legacy and `r g b / a` syntax), `hsl()/hsla()`, all CSS named colors, `transparent`,
  `currentColor` (borders follow `color` by default). Borders: per-side width, color and style (`solid`, `dashed`, `dotted`) with mitred corners.
  `text-transform`, `letter-spacing`, `box-shadow` (several, blurred, spread, `inset`; `examples/inset-shadow.html`) and painted `opacity` (a layer
  over the whole subtree) and `position: relative|absolute` (against the nearest positioned ancestor), `fixed` and `sticky` (`examples/fixed.html`) with `top/right/bottom/left/inset` and `z-index` (see
  `examples/position.html`) `linear-gradient`/`radial-gradient` and their `repeating-` forms as `background`
  (`examples/gradients.html`), `transition` (any animatable property or `all`, with delay, `ease`/`linear`/`cubic-bezier()`/`steps()`; a change mid-transition continues from where it is) and `@keyframes` `animation` (with `transitionend`, `animationstart` and `animationend` events carrying `propertyName`/`animationName`/`elapsedTime`; `examples/anim-events.html`; no `transitioncancel`/`animationiteration`) (iterations, `infinite`, direction, fill modes, delays; `examples/animations.html`) over opacity, colors, lengths, padding/border widths, radius, `transform` and more, `transform` (`translate`, `scale`, `rotate`, `skew`, `matrix`, with `transform-origin`; hit testing and `getBoundingClientRect` follow it; `examples/transforms.html`), `<img>` (PNG/JPEG/WebP/GIF from files next to the page or `data:` URLs; sized by CSS, `width`/`height` attributes or the picture) and `background-image: url()` with `background-size`/`-position`/`-repeat` (`examples/images.html`), `float`/`clear` (`examples/float.html`), `<include src media>` (fragments spliced in while loading, with fallback content; `examples/include.html`), `<frame>`/`<iframe>` (a separate document with its own scripts, scrolling, focus and input, painted into the box; `src`, `loadFile`/`loadHtml`/`loadEmpty`, `frame.document.$()`, `newdocument`/`complete`, `postMessage` both ways with `window.parent`; `content-style`; `examples/frames.html`), accessibility for screen readers (UI Automation, VoiceOver, AT-SPI through AccessKit; roles, names, states and actions from the DOM and ARIA attributes; also as JSON from the C API; see `docs/guide/accessibility.md`), `new Window({...})` and `Window.this.modal({...})` (several native windows, each its own page; messages, `close(value)`, `Window.share`, `parameters`, a Promise-returning modal; `examples/windows.html`), `Window.this.selectFile`/`selectFolder`/`modal(<alert>…)` (native open/save/folder choosers and message boxes through AppleScript, PowerShell or `zenity`/`kdialog`, or opt-in in-window message boxes with rich content, custom buttons and CSS: `<question inline>`/`Window.this.messageBoxes = "inline"`, returning a Promise; `examples/dialogs.html`), `element.popup()`/`takeOff()` with `popup-position`/`popup-animation` CSS, `title`/`tooltip` tooltips with `tooltiprequest`, and `contextmenu` with `<menu.context>` (painted in the window, or in an OS window of their own with `window: "popup"`; `examples/popups.html`), `<frameset cols|rows>` with `<splitter>`s (panes are frames or any block; drag to resize; `frameset.state`; `examples/frameset.html`), `white-space` (`normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line`, with tab stops), lists (`ul`/`ol`/`li` bullets and counters, `::marker` and `list-style-position` (`examples/markers.html`), `list-style-type` incl. `decimal`, `lower-alpha`, `upper-roman`, `start`/`reversed`/`value`), `:has()`, `text-overflow: ellipsis`, `<hr>`, `<fieldset>`, `<details>`/`<summary>` (click toggles, `toggle` event), `<progress>`, `<meter>`, `<input type=range>` (drag, arrows, Home/End; `input`/`change`), `<input type=number>` (spin arrows, Up/Down by `step` within `min`/`max`, only number characters accepted; `examples/number.html`), `<input type=color>` (a swatch plus an editable hex value, normalised to `#rrggbb`), `date`/`time`/`month`/`datetime-local` (typed digits only, Up/Down steps the field under the caret, wrapping; `min`/`max`), and `<input type=file>` (a "Choose File" button; the `nile` binary opens the system chooser through AppleScript, PowerShell or `zenity`/`kdialog`; scripts see `files` with `name`/`size`/`type`/`text()`; C hosts answer `nile_view_take_file_request` with `nile_view_choose_files`; `examples/inputs.html`), `cursor` (pointer shapes, inherited; links, inputs and disabled controls have defaults), tables (`table`, row groups, `tr`, `td`/`th` with `colspan`/`rowspan`, `caption`, `border-spacing`, `border-collapse`, `vertical-align`/`valign`; `examples/tables.html`), inline `vertical-align` (`sub`, `super`, lengths; `<sub>`/`<sup>`; `examples/subsup.html`), `display: grid` (`grid-template-columns/rows` with `px`/`%`/`fr`/`auto`/`minmax()`/`repeat()` incl. `auto-fill`/`auto-fit`,
  `grid-auto-*`, `grid-column/row` with spans and line numbers, `grid-template-areas` + `grid-area`, `row-gap`/`column-gap`,
  `justify-items`/`justify-self`/`align-self`/`align-content`/`place-*`; `examples/grid.html`), and `::before`/`::after` with `content` (strings, `attr()`, quotes; any `display`, including
  inline-block and absolute; `examples/pseudo.html`) too. See `examples/css-compat.html` for a page that checks these.

## Known gaps (deliberate, for now)

- Font fallback: system fallback covers scripts (CJK, ...) but not every symbol block, so Dingbats (`✓`) and
  `⚠`-range symbols name Apple Symbols / Zapf Dingbats / Segoe UI Symbol / Noto Sans Symbols explicitly. Only
  checked on macOS; other symbol blocks may still show a missing-glyph box.
- **Inline layout is partial.** Shrink-to-fit `inline-block` uses max-content (it won't narrow to fit a
  line), inline margins are ignored, a wrapped inline's border/radius isn't sliced at the break, only the
  innermost decorated inline is painted, and block children of an inline element don't split the run.
- Window gaps: no multiple windows, dialogs or `Window.this`-style script API (I couldn't verify Sciter's shape
  from its docs), no `window-material`/`window-corners`, `solid-with-shadow` has no shadow, and `transparent` shows the
  desktop only where the platform's surface supports alpha (softbuffer doesn't expose an alpha mode).
- Editing gaps: no spell-check, drag-and-drop of text or `contenteditable="plaintext-only"` (treated as `true`); paste takes the clipboard's HTML when it has some (bold, italic, underline, links, headings, lists, quotes, images and a short list of inline styles survive; everything else is dropped; `nile-capi` hosts only exchange text) and copy writes HTML and text; undo is a snapshot of the whole editing host, so a script that rewrites the host also rewrites its history; `Range` has `cloneContents`/`extractContents`/`compareBoundaryPoints`/`comparePoint`/`intersectsNode`, but no `Range.getClientRects` or `surroundContents` across blocks, and fragments are `<div>`s marked as `nodeType` 11; `execCommand("insertTable", false, "3x2")` inserts a table (Tab and Shift+Tab move between cells, Tab in the last cell adds a row, Enter stays in the cell, Backspace and Delete don't merge cells, deleting across cells empties them), `tableInsertRowAbove/Below`, `tableInsertColumnLeft/Right`, `tableDeleteRow/Column/Table`, `tableMergeCells` and `tableSplitCell` edit the table around the caret (spans are kept consistent; `examples/image-editing.html`) but columns can't be resized by dragging, a click selects an image (outline, eight handles; corners keep the proportions, Shift frees them; one undo step) but there is no image wrapping or alignment, and `fontSize` 1-7 maps to a fixed px table; indent uses `margin-left` rather than nested lists; Page Up/Down don't move the caret; selection inside `<input>`/`<textarea>` is separate from the document selection.
- Selection gaps: page text selection works across runs in paint order (so `z-index` reordering can scramble the
  selection order), can't start inside a link or control, has no keyboard (Shift+arrows), and ignores `user-select: none` on a span inside a selectable block (it applies per paragraph/run); `text-selection-color` isn't painted. Rich-text `Range`/`Selection` APIs are described above. Form gaps: no drag-and-drop; no `<form>` submit or validation; list boxes have no focus ring or
  `Ctrl`-arrow cursor, and the drop-down closes when the page scrolls; Home/End in a
  `<textarea>` use hard line breaks only; `selectionStart/End` count characters, not UTF-16 units; `value` lives
  in the `value` attribute (no separate default value). Sciter's `Element.value` for other behaviors isn't there.
- Scrollbars are thin overlay bars (draggable; clicking the track jumps there), with no `scrollbar-*` styling, no
  hover/fade behavior and no arrow buttons; clipping ignores `border-radius`.
- JS gaps: no `Blob`/`FormData`/streams/`WebSocket`, no cookies or CORS, `fetch` follows redirects only for http(s), `@sys`/`@debug`/`@storage` modules and bare module specifiers (only relative and absolute paths plus `@nile`/`@sciter`/`@env`; `setModuleUrlResolver` is accepted but unused, `compress`/`decompress`/`loadLibrary`/the synchronous `import()` throw),
  `offsetParent` (approximated by the parent).
  From Sciter: class components, `componentUpdate`,
  element-level `onGlobalEvent`, `Element.value` (no native behaviors yet), `this://` URLs, and JSX shortcuts like `.class`;
  `patch` is simplified (children match by key, id, then tag).
- CSS gaps: tables are laid out as a grid, so `table-layout: fixed` (with a table `width`; column widths from `<col>` or the first row's cells, px or %; `examples/table-fixed.html`) is the only place column widths are honoured: automatic tables size columns from their content only, and there's no `caption-side`, row-group backgrounds or collapsed-border conflict resolution (each cell keeps its own borders and they overlap), and `display: table-*` on stray elements isn't wrapped in anonymous tables; accessibility: no live regions, text selection or caret positions, `aria-owns`/`aria-controls`, and it has not been run against a real screen reader yet; windows: owned windows (dialogs, tools, popups) stay above their owner on macOS and Windows only (the Windows path is untested), `modal(new Window(...))` and a blocking `modal({url})` aren't possible, `Window.all` only knows the windows a page opened, C hosts can't open windows yet; popup windows don't close when another application is clicked, they aren't clamped to the monitor, and tooltips stay inside their window; frames: no `:busy` (loading is synchronous), a frame is painted over everything else in the page (so a drop-down or fixed box of the outer page can't cover it), isn't clipped by an ancestor's `overflow` or moved by its transform (only its rectangle follows scrolling), Tab doesn't enter it, and scripts cross the boundary as JSON (no shared DOM objects); `float`/`clear` work on boxes, but text next to a float keeps the narrowed width for the whole paragraph instead of widening below it; named grid lines, `grid-template` shorthand, `subgrid`, `counter()`/`url()` in `content`, other pseudo-elements (`::first-line`, `::placeholder`, ...); `fixed` is pinned to the viewport (a clipping ancestor still clips it) and `sticky` slides within its parent for the page's scroll only (not inside an `overflow` container; vertical insets only), an absolute box with `auto` insets sits at its static position approximated as the start of its parent's content box, or below the previous in-flow element sibling (not after preceding text or inline content; `examples/static-position.html`), stacking contexts follow the spec for positioned boxes, `z-index`, `opacity`, `transform` and flex/grid items (`examples/stacking.html`), except that a positioned box inside an `overflow`, `opacity`, `transform` or `sticky` ancestor is painted with that ancestor instead of being lifted past it, and `isolation`/`will-change`/`mix-blend-mode` don't create contexts; multiple background layers, `background-origin`/`-clip`/`-attachment`, `object-fit`, SVG, `<img>` alt text, remote (`http`) images,
  3D transforms are flattened onto the page (`rotateX/Y/3d`, `translateZ/3d`, `scale3d`, `perspective()`, `matrix3d`, and a container's `perspective`/`perspective-origin`; hit testing and `getBoundingClientRect` follow the projection) but there is no `transform-style: preserve-3d`, `backface-visibility` or depth sorting, so nested 3D scenes paint in tree order; `double`/`groove` borders draw solid; `calc()` percentages
  resolve against the parent's laid-out content size (heights only when the element sets its own height);
  `@supports`/`@font-face` are skipped; `@media` has no `prefers-*` beyond light/no-preference and no
  `resolution`.
- `border-radius` takes one to four values, `%`, and the per-corner longhands; elliptical radii after a `/` are read as circular (`examples/radius.html`). `:visited` isn't supported.
- HTML gaps: `list-style-image`; `list-style-position: inside` reserves room in the item's left padding (so wrapped lines align with the first line's text rather than the marker) and `::marker` supports `color`, font properties and `content` (a string, or `""` to hide) but not `::marker` backgrounds or sizes beyond the font; inline `vertical-align` only shifts the glyphs (the line box doesn't grow, `middle`/`top`/`bottom` on inline boxes are ignored, an underline follows the shifted text), `<legend>` doesn't sit on the `<fieldset>` border, `date`/`time`/`month`/`datetime-local` inputs are text fields with field stepping but no calendar or clock popup, `type=color` has no picker popup (type a hex value), `type=number` has no wheel stepping, `valueAsNumber` or validation, and `type=file` has no drag-and-drop or `webkitdirectory`, bare text inside a closed `<details>` stays visible, `text-overflow: ellipsis` is for a single clipped line only.
- Sibling and `:nth-child` matching scans child lists, so very wide lists restyle slowly.
- Variable-font axes aren't applied when drawing glyphs.
- CPU rasterization (`softbuffer`) — fine for now, GPU comes later.

## Roadmap

Done so far: the full pipeline (HTML, CSS, layout, Skia painting, QuickJS scripting), forms and
text editing (undo/redo, selection, drop-downs, list boxes), scrolling, images, gradients, grid,
float, tables, transforms, transitions and animations, and opt-in page text selection.

1. **Incremental updates** (mostly done) — `cursor` works, and a restyle that only changes paint (text
   and background colors, borders colors, shadows, radius, transform and opacity amounts) keeps the
   laid-out boxes instead of relaying out, so hover effects and animations of those properties skip
   layout. A pointer or focus change that no selector mentions (`:hover`, `:active`, `:focus`) skips
   the restyle too. Still open: restyling only the affected elements (a hover on a page that does
   have `:hover` rules re-cascades everything, ~7 ms for 4,500 nodes), and keeping the layout when
   an element gains or loses a transform or opacity, when text color changes on pages with
   `::before`/`::after`, or for inline elements' backgrounds. `NILE_TRACE=1` prints per-phase timings.
2. **Embedding API** (C ABI done; see above) — still to do: a GPU surface path, multi-button and touch input. Language bindings (Go, C#, Python) come last, once the engine covers nearly everything.
3. **Sciter compatibility** — `@sys` module, class components and
   `componentUpdate`, `this://` URLs.
4. **GPU** — Skia on Metal
   (macOS), D3D (Windows), Vulkan/GL (Linux) instead of the CPU raster path.
5. **Accessibility** — `accesskit`.
   (Window configuration, frameless windows and window chrome roles are done.)
6. **CSS and control polish** — text wrapping around floats line by line; a paged print-layout view in a window, repeating table headers across pages (the page breaks, `@page` size/margins/margin boxes and PDF export are done: `--pdf`, `--page N`). The known gaps above list the rest.

## License

MIT OR Apache-2.0
