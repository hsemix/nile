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
cargo test --workspace
```

## What's supported today

- **Selectors** (Servo's `selectors` crate): type, `.class`, `#id`, attributes (`[a]`, `[a^=b]`, ...), the
  descendant / `>` / `+` / `~` combinators, `:first-child`, `:last-child`, `:nth-child()`, `:not()`, `:is()`,
  `:root`, `:empty`, and `:hover` / `:active` / `:focus` (driven by `InteractionState`; nothing sets it
  until events land). Rules with unsupported selectors (pseudo-elements, `:visited`, ...) are dropped.
- **Properties:** `display` (block, inline, inline-block, flex, none), `box-sizing`, `width/height` + min/max, `margin`, `padding`,
  `border`, `border-radius`, `background(-color)`, `color`, `font-size/weight/family`,
  `line-height`, `text-align`, `font-style`, `text-decoration` (underline, line-through), `flex`, `flex-direction/wrap/grow/shrink/basis`,
  `justify-content`, `align-items`, `gap`.
- **JavaScript DOM** (QuickJS): `document`, `Element`/`Node`/`Text`, `querySelector(All)`, `matches`, `closest`,
  `classList`, `style`, `dataset`, attributes, `innerHTML`/`textContent`, `createElement`, tree edits,
  `addEventListener` with capture/bubble/once, `on*` properties and attributes, `click`/`mouse*`/`key*`/`focus`
  events, `DOMContentLoaded`/`load`, timers, `requestAnimationFrame`, promises, `<script src>` (see
  `examples/scripting.html`). Layout queries: `getBoundingClientRect`, `offset*`/`client*`, `getComputedStyle`
  (common properties), `innerWidth/Height`, `scrollTo`/`scrollY`/`scrollIntoView` (see `examples/measure.html`).
- **Pointer interaction:** hit testing (down to the inline element under the pointer), `:hover`, `:active` and
  click-to-focus on links and form controls, restyled live (see `examples/interact.html`).
- **Inline content:** mixed text, `<br>`, inline padding/border/background/radius and `inline-block` (with
  baseline) wrap as one flow (see `examples/inline.html`, `examples/selectors.html`).
- **Units:** `px`, `em`, `rem`, `%`, `pt`. **Colors:** hex, `rgb()/rgba()`, basic names.

## Known gaps (deliberate, for now)

- **Inline layout is partial.** Shrink-to-fit `inline-block` uses max-content (it won't narrow to fit a
  line), inline margins are ignored, a wrapped inline's border/radius isn't sliced at the break, only the
  innermost decorated inline is painted, and block children of an inline element don't split the run.
- JS gaps: `fetch`, modules/JSX, `window.location`, `devicePixelRatio` (always 1), element-level scrolling
  (`scrollTop` only works on the page), `offsetParent` (approximated by the parent). Sciter's own DOM
  extensions aren't there either.
- `border-radius` takes one value (no per-corner radii); `:has()`, `::before/::after` and `:visited` aren't supported.
- Sibling and `:nth-child` matching scans child lists, so very wide lists restyle slowly.
- `opacity` is parsed but not painted (needs stacking contexts in the paint list).
- Variable-font axes aren't applied when drawing glyphs.
- CPU rasterization (`softbuffer`) — fine for now, GPU comes later.

## Roadmap

1. **Tab order + incremental updates** — Tab focus order, the `cursor` property, and restyling /
   relaying out only what a state change affects (today every change restyles the whole page).
2. **JSX/module loader + Sciter extensions** — `import`/JSX in scripts, and Sciter-shaped extras (`$()`,
   `Element.state`, `Element.value`, ...) so Sciter apps can port.
3. **Stacking contexts + GPU** — paint tree with opacity, transforms, clipping,
   overflow scrolling; Skia on Metal (macOS), D3D (Windows), Vulkan/GL (Linux).
4. **Embedding API** — a C ABI (`cbindgen`) so Go, C#, Python, etc. can host it.
5. **Form controls, images, JSX in the module loader, accessibility (`accesskit`).**

## License

MIT OR Apache-2.0
