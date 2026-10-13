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
dragging). Scripts and host call each other: `nile_view_eval`, `nile_view_call`,
`nile_view_register_function` (scripts see it as `host.name(...)`, JSON in and out) and
`nile_view_take_console`. The clipboard is yours through `nile_view_set_clipboard`.
Single-threaded; no panic crosses the boundary. See `examples/c/render.c` for a complete program
that renders a page to an image:

```sh
cargo build -p nile-capi --release
cc examples/c/render.c -Icrates/nile-capi/include -Ltarget/release -lnile -Wl,-rpath,$PWD/target/release -o render
./render examples/hello.html out.ppm
```

Not in the C API yet: multiple mouse buttons, touch, IME composition, GPU surfaces (it paints
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

An interactive ribbon with seven tabs, grouped commands, navigation, a document
preview and a status bar. Font, style and alignment commands format the Overview
paragraph; formatting undo/redo, document search and zoom also work. Other commands
report a design-preview message. This is a UI example, not a document editor;
save, sharing and clipboard commands do not persist or transfer content.

## Desktop application showcase

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
  (`examples/fetch.html`). Also `MutationObserver`, `URL`/`URLSearchParams`, `localStorage`/`sessionStorage`
  (in memory, not saved), `matchMedia`, `structuredClone`, `atob`/`btoa` and `TextEncoder`/`TextDecoder`.
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
  more room above. `<select multiple>` and `size > 1` are list boxes drawn inline: click, Cmd/Ctrl-click to toggle,
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
  `text-transform`, `letter-spacing`, outer `box-shadow` (several, blurred, spread) and painted `opacity` (a layer
  over the whole subtree) and `position: relative|absolute` (against the nearest positioned ancestor), `fixed` and `sticky` (`examples/fixed.html`) with `top/right/bottom/left/inset` and `z-index` (see
  `examples/position.html`) `linear-gradient`/`radial-gradient` and their `repeating-` forms as `background`
  (`examples/gradients.html`), `transition` (any animatable property or `all`, with delay, `ease`/`linear`/`cubic-bezier()`/`steps()`; a change mid-transition continues from where it is) and `@keyframes` `animation` (with `transitionend`, `animationstart` and `animationend` events carrying `propertyName`/`animationName`/`elapsedTime`; `examples/anim-events.html`; no `transitioncancel`/`animationiteration`) (iterations, `infinite`, direction, fill modes, delays; `examples/animations.html`) over opacity, colors, lengths, padding/border widths, radius, `transform` and more, `transform` (`translate`, `scale`, `rotate`, `skew`, `matrix`, with `transform-origin`; hit testing and `getBoundingClientRect` follow it; `examples/transforms.html`), `<img>` (PNG/JPEG/WebP/GIF from files next to the page or `data:` URLs; sized by CSS, `width`/`height` attributes or the picture) and `background-image: url()` with `background-size`/`-position`/`-repeat` (`examples/images.html`), `float`/`clear` (`examples/float.html`), `white-space` (`normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line`, with tab stops), lists (`ul`/`ol`/`li` bullets and counters, `list-style-type` incl. `decimal`, `lower-alpha`, `upper-roman`, `start`/`reversed`/`value`), `:has()`, `text-overflow: ellipsis`, `<hr>`, `<fieldset>`, `<details>`/`<summary>` (click toggles, `toggle` event), `<progress>`, `<meter>`, `<input type=range>` (drag, arrows, Home/End; `input`/`change`), `<input type=number>` (spin arrows, Up/Down by `step` within `min`/`max`, only number characters accepted; `examples/number.html`), `cursor` (pointer shapes, inherited; links, inputs and disabled controls have defaults), tables (`table`, row groups, `tr`, `td`/`th` with `colspan`/`rowspan`, `caption`, `border-spacing`, `border-collapse`, `vertical-align`/`valign`; `examples/tables.html`), inline `vertical-align` (`sub`, `super`, lengths; `<sub>`/`<sup>`; `examples/subsup.html`), `display: grid` (`grid-template-columns/rows` with `px`/`%`/`fr`/`auto`/`minmax()`/`repeat()` incl. `auto-fill`/`auto-fit`,
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
- Editing gaps: no IME, spell-check, drag-and-drop of text or `contenteditable="plaintext-only"` (treated as `true`); paste is plain text only; undo is a snapshot of the whole editing host, so a script that rewrites the host also rewrites its history; `Range` has no `cloneContents`/`extractContents`/`compareBoundaryPoints`; `execCommand` has no `insertTable`, and `fontSize` 1-7 maps to a fixed px table; indent uses `margin-left` rather than nested lists; Page Up/Down don't move the caret; selection inside `<input>`/`<textarea>` is separate from the document selection.
- Selection gaps: page text selection works across runs in paint order (so `z-index` reordering can scramble the
  selection order), can't start inside a link or control, has no keyboard (Shift+arrows), and ignores `user-select: none` on a span inside a selectable block (it applies per paragraph/run); `text-selection-color` isn't painted; `Range`/
  `Selection` objects beyond `toString()`/`removeAllRanges()` are missing. Form gaps: no IME or drag-and-drop; no `<form>` submit or validation; `<select>` has no `<optgroup>` labels, list boxes have no drag-selection, focus ring or
  `Ctrl`-arrow cursor, and the drop-down closes when the page scrolls; Home/End in a
  `<textarea>` use hard line breaks only; `selectionStart/End` count characters, not UTF-16 units; `value` lives
  in the `value` attribute (no separate default value). Sciter's `Element.value` for other behaviors isn't there.
- Scrollbars are thin overlay bars (draggable; clicking the track jumps there), with no `scrollbar-*` styling, no
  hover/fade behavior and no arrow buttons; clipping ignores `border-radius`.
- JS gaps: no `Blob`/`FormData`/streams/`WebSocket`, `ResizeObserver`/`IntersectionObserver`, no persistence for `localStorage`, no cookies or CORS, `fetch` follows redirects only for http(s), bare/`@sciter`/`@env`-style module specifiers (only relative and absolute paths),
  `window.location`, `devicePixelRatio` (always 1), `offsetParent` (approximated by the parent).
  From Sciter: class components, `componentUpdate`,
  `onGlobalEvent`, `Element.value` (no native behaviors yet), `this://` URLs, and JSX shortcuts like `.class`;
  `patch` is simplified (children match by key, id, then tag).
- CSS gaps: tables are laid out as a grid, so there's no `table-layout: fixed`, percentage column widths, `<col>` widths, `caption-side`, row-group backgrounds or collapsed-border conflict resolution (each cell keeps its own borders and they overlap), and `display: table-*` on stray elements isn't wrapped in anonymous tables; `float`/`clear` work on boxes, but text next to a float keeps the narrowed width for the whole paragraph instead of widening below it; named grid lines, `grid-template` shorthand, `subgrid`, `counter()`/`url()` in `content`, other pseudo-elements (`::first-line`, `::placeholder`, `::marker`, ...); `fixed` is pinned to the viewport (a clipping ancestor still clips it) and `sticky` slides within its parent for the page's scroll only (not inside an `overflow` container; vertical insets only), an absolute box with `auto` insets sits at its containing block's corner when that isn't its parent (not at its static position), `z-index` only reorders siblings (no isolated stacking contexts, flex items ignore it); `inset` shadows, multiple background layers, `background-origin`/`-clip`/`-attachment`, `object-fit`, SVG, `<img>` alt text, remote (`http`) images,
  3D transforms; `double`/`groove` borders draw solid; `calc()` percentages
  resolve against the parent's laid-out content size (heights only when the element sets its own height);
  `@supports`/`@font-face` are skipped; `@media` has no `prefers-*` beyond light/no-preference and no
  `resolution`.
- `border-radius` takes one value (no per-corner radii); `:visited` isn't supported.
- HTML gaps: `list-style-position: inside` and `list-style-image` (markers always hang outside; a `<summary>`'s triangle is inside), no `::marker` styling, inline `vertical-align` only shifts the glyphs (the line box doesn't grow, `middle`/`top`/`bottom` on inline boxes are ignored, an underline follows the shifted text), `<legend>` doesn't sit on the `<fieldset>` border, `<input type=date|color|file>` are plain text boxes (`type=number` has no wheel stepping, `valueAsNumber` or validation), bare text inside a closed `<details>` stays visible, `text-overflow: ellipsis` is for a single clipped line only.
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
2. **Embedding API** (C ABI done; see above) — still to do: a GPU surface path, multi-button and touch input, IME. Language bindings (Go, C#, Python) come last, once the engine covers nearly everything.
3. **Sciter compatibility** — `@env`/`@sciter`/`@sys` modules, class components and
   `componentUpdate`, `this://` URLs.
4. **Stacking contexts + GPU** — real stacking contexts; Skia on Metal
   (macOS), D3D (Windows), Vulkan/GL (Linux) instead of the CPU raster path.
5. **Accessibility and input** — `accesskit`, IME.
   (Window configuration, frameless windows and window chrome roles are done.)
6. **CSS and control polish** — date/color/file inputs, `::marker`, 3D transforms, `inset` shadows, text wrapping around floats line by line, `table-layout: fixed`,
   `<optgroup>`, list-box drag selection. The known gaps above list the rest.

## License

MIT OR Apache-2.0
