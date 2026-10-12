# nile

An open-source, embeddable HTML/CSS/JS UI engine in Rust, in the spirit of
[Sciter](https://sciter.com): its own renderer, no webview, small enough to
ship inside a desktop app.

*"nile" is a working title.*

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
| `nile-script` | JS runtime (`console.log` for now) | `rquickjs` |
| `nile-shell` | Native window, HiDPI, scrolling, headless PNG | `winit`, `softbuffer` |

## Running it

Requires recent stable Rust. On macOS you also need the Xcode command line
tools (`xcode-select --install`). The first build downloads prebuilt Skia
binaries from GitHub (macOS arm64/x86_64, Windows, Linux).

```sh
cargo run -p nile-shell -- examples/hello.html      # window: R reloads, wheel scrolls
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

## What's supported today

- **Selectors** (Servo's `selectors` crate): type, `.class`, `#id`, attributes (`[a]`, `[a^=b]`, ...), the
  descendant / `>` / `+` / `~` combinators, `:first-child`, `:last-child`, `:nth-child()`, `:not()`, `:is()`,
  `:root`, `:empty`, and `:hover` / `:active` / `:focus` (driven by `InteractionState`; nothing sets it
  until events land). Rules with unsupported selectors (`:visited`, `::first-line`, ...) are dropped.
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
  focus order (`tabindex` respected), and `input`/`change`/`focus`/`blur` events. Text selection in inputs and
  textareas: Shift+arrows/Home/End, drag, Shift+click, double-click (word), triple-click (all), Ctrl/Cmd+A, and
  copy/cut/paste through the system clipboard; Tab selects an input's contents. In JS: `selectionStart`,
  `selectionEnd`, `setSelectionRange()`, `select()`. `<select>` with `<option>`s: a drop-down that opens on click,
  Space/Enter or Alt+Down, with mouse hover, arrows/Home/End, type-ahead, Escape, disabled options, and `input`/
  `change` events; sized to its longest option. In JS: `value`, `selectedIndex`, `options`, `option.selected`/
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
  over the whole subtree) and `position: relative|absolute` (against the nearest positioned ancestor) with `top/right/bottom/left/inset` and `z-index` (see
  `examples/position.html`) `linear-gradient`/`radial-gradient` and their `repeating-` forms as `background`
  (`examples/gradients.html`), `<img>` (PNG/JPEG/WebP/GIF from files next to the page or `data:` URLs; sized by CSS, `width`/`height` attributes or the picture) and `background-image: url()` with `background-size`/`-position`/`-repeat` (`examples/images.html`), `float`/`clear` (`examples/float.html`), `display: grid` (`grid-template-columns/rows` with `px`/`%`/`fr`/`auto`/`minmax()`/`repeat()` incl. `auto-fill`/`auto-fit`,
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
- Form gaps: selection works only inside inputs/textareas (no selecting page text); no undo/redo, word-wise
  movement (Alt/Ctrl+arrows), IME or drag-and-drop; no `<form>` submit or validation; `<select>` is single-choice only (no `multiple`, `size`, `<optgroup>` labels), its
  list doesn't scroll or flip above the select near the bottom edge, and it closes when the page scrolls; Home/End in a
  `<textarea>` use hard line breaks only; `selectionStart/End` count characters, not UTF-16 units; `value` lives
  in the `value` attribute (no separate default value). Sciter's `Element.value` for other behaviors isn't there.
- Scrollbars are overlay indicators only: no dragging or track clicks, no `scrollbar-*` styling, no keyboard
  scrolling (Page Up/Down, arrows); clipping ignores `border-radius`; `white-space: pre`/`pre-wrap` don't preserve
  whitespace yet (`pre` just doesn't wrap).
- JS gaps: `fetch`, bare/`@sciter`/`@env`-style module specifiers (only relative and absolute paths),
  `window.location`, `devicePixelRatio` (always 1), element-level scrolling (`scrollTop` only works on the
  page), `offsetParent` (approximated by the parent). From Sciter: class components, `componentUpdate`,
  `onGlobalEvent`, `Element.value` (no native behaviors yet), `this://` URLs, and JSX shortcuts like `.class`;
  `patch` is simplified (children match by key, id, then tag).
- CSS gaps: no tables; `float`/`clear` work on boxes, but text next to a float keeps the narrowed width for the whole paragraph instead of widening below it; named grid lines, `grid-template` shorthand, `subgrid`, `counter()`/`url()` in `content`, other pseudo-elements (`::first-line`, `::placeholder`, ...) or `:has()`; `fixed` acts like `absolute` against the page (it scrolls), `sticky` like `relative`, an absolute box with `auto` insets sits at its containing block's corner when that isn't its parent (not at its static position), `z-index` only reorders siblings (no isolated stacking contexts, flex items ignore it); `inset` shadows, multiple background layers, `background-origin`/`-clip`/`-attachment`, `object-fit`, SVG, `<img>` alt text, remote (`http`) images, transforms,
  transitions; `double`/`groove` borders draw solid; `calc()` percentages
  resolve against the parent's laid-out content size (heights only when the element sets its own height);
  `@supports`/`@font-face`/`@keyframes` are skipped; `@media` has no `prefers-*` beyond light/no-preference and no
  `resolution`.
- `border-radius` takes one value (no per-corner radii); `:has()` and `:visited` aren't supported.
- Sibling and `:nth-child` matching scans child lists, so very wide lists restyle slowly.
- Variable-font axes aren't applied when drawing glyphs.
- CPU rasterization (`softbuffer`) — fine for now, GPU comes later.

## Roadmap

1. **Incremental updates** — the `cursor` property, and restyling /
   relaying out only what a state change affects (today every change restyles the whole page).
2. **More controls + Sciter compatibility** — selecting page text, undo, `<select multiple>`, draggable scrollbars and keyboard scrolling, `@env`/`@sciter`/`@sys` modules, class components and `componentUpdate`, `this://` URLs.
3. **Stacking contexts + GPU** — paint tree with opacity, transforms, clipping,
   overflow scrolling; Skia on Metal (macOS), D3D (Windows), Vulkan/GL (Linux).
4. **Embedding API** — a C ABI (`cbindgen`) so Go, C#, Python, etc. can host it.
5. **Form controls, images, JSX in the module loader, accessibility (`accesskit`).**

## License

MIT OR Apache-2.0
