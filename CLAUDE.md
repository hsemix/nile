# Nile

An open-source, embeddable HTML/CSS/JS UI engine in Rust, in the spirit of
Sciter: its own renderer, **no webview**, small enough to ship inside desktop
apps. The motivation is to give Sciter's well-designed approach an open-source
home that doesn't depend on one person. Read `README.md` for the supported
CSS, known gaps and roadmap.

## Goals that shape decisions

- **Own rendering pipeline.** Never fall back to a system webview or Chromium.
- **Sciter compatibility where sensible.** When designing JS/DOM APIs and CSS
  extensions, follow Sciter's shapes so existing Sciter apps can port.
  Prefer web standards where Sciter matches them.
- **Cross-platform.** Development happens on macOS; Windows and Linux must keep
  working. CI runs on all three. No platform-specific code outside the shell
  (and later, GPU backends) without a `cfg` fallback.
- **Small and embeddable.** Watch binary size and dependency weight. Prefer
  focused crates over large frameworks. Will expose a C ABI later.
- **Rust only** for the engine.

## Layout

```
crates/
  nile-dom     arena DOM (NodeId into Vec<Node>), html5ever parsing
  nile-style   CSS parsing (cssparser), cascade, inheritance, UA stylesheet
  nile-text    Parley wrapper: shaping, line breaking, font fallback
  nile-layout  styled DOM -> Taffy tree, text measurement, flattened boxes
  nile-render  paints LayoutTree onto a Skia canvas
  nile-script  QuickJS via rquickjs; native DOM bridge + prelude.js (web DOM classes)
  nile-shell   `nile` binary: winit + softbuffer window, headless --png mode
examples/      test pages
docs/          README screenshot
```

Data flows one way: dom -> style -> layout (uses text) -> render. Keep crate
dependencies in that direction. `nile-render` must not depend on `nile-dom`.

## Commands

```sh
cargo run -p nile-shell -- examples/hello.html                 # window; R reloads, wheel scrolls
cargo run -p nile-shell -- examples/hello.html --png out.png --scale 2
cargo run -p nile-shell -- examples/interact.html --png out.png --hover 40,40   # or --press / --click X,Y
cargo run -p nile-shell -- examples/scripting.html --png out.png --click 60,80 --wait 400
cargo run -p nile-shell -- examples/measure.html --png out.png
cargo run -p nile-shell -- examples/todo/index.html --png out.png --click 60,128   # modules + JSX
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings          # CI enforces this
```

## Working conventions

- **Check visual changes by rendering.** After any layout, style, text or paint
  change, render the relevant example with `--png` and look at the image.
  Tests passing isn't enough for visual work.
- **Add an example page** in `examples/` for each new feature (e.g.
  `examples/inline.html`) so it can be checked visually.
- **Unit tests** go next to the code (`#[cfg(test)]`). Layout tests assert box
  rects. Use `margin: 0` on body to keep numbers simple.
- **Unsupported CSS is ignored silently**, like a browser. Never panic on bad input.
- Mark known shortcuts with `TODO:` and say what the real fix is.
- Update the README's "supported" and "known gaps" sections when they change.
- Pin dependency versions in the root `Cargo.toml` `[workspace.dependencies]`;
  crates use `.workspace = true`. Commit `Cargo.lock`.

## Gotchas

- The document is `Rc<RefCell<Document>>`, shared by the shell and the script
  host. Never hold a borrow across a call into scripts (`ScriptHost::dispatch`,
  `tick`, `run`): they borrow it mutably. Scripts bump `Document::version`; the
  shell's `Page::sync` restyles when it changed. Detached nodes stay in the arena.
- Scripts read layout through `nile_script::Environment`, implemented by the
  shell's `Env` over the shared `View` (styles, layout, viewport, scroll). Each
  query syncs and relays out first, so don't hold a `View` borrow while calling
  into scripts. Native `Option::None` reaches JS as `undefined`, not `null` —
  normalise in `prelude.js`.
- Boxes are `content-box` unless `box-sizing: border-box` (Taffy's own default
  is border-box, so `taffy_style` sets it explicitly).
- Sciter helpers in `prelude.js` follow docs.sciter.com (verified Oct 2026):
  `on()` handlers get `(evt, matched)` with `this` = the attached element; JSX
  returns vnode arrays that `append`/`content`/`patch` render. Check the docs
  before adding more, don't write Sciter API shapes from memory.
- JSX is a source transform (`nile-script/src/jsx.rs`, a scanner, not a parser)
  applied to every script and module before QuickJS sees it; it returns `None`
  for non-JSX or malformed input so the engine reports the original error. Modules
  resolve relative to the importing file (inline modules are named
  `page.html#moduleN`).
- DOM wrappers in JS are keyed by arena id (`wrap(id)` in `prelude.js`); new
  DOM API usually means one small native in `lib.rs` plus a JS method.

- Taffy rounds layout to whole pixels. Final text line breaking uses
  `rect.width + 1.0` so rounding doesn't cause spurious wraps.
- The page's canvas background comes from `html`, else `body` (CSS rule),
  not from the body box.
- softbuffer wants 0RGB u32 pixels. Skia renders `BGRA8888` into the same
  bytes, which matches on little-endian (all targets we support).
- Selector matching is Servo's `selectors` crate (MPL-2.0 — keep it a separate
  dependency, don't copy its code). A rule whose selector list fails to parse is
  dropped whole. `ElementRef` in `nile-style/src/selector.rs` adapts the arena DOM.
- Inline padding/border become no-break-space "spacer" runs sized via the font's
  NBSP advance (`TextEngine::nbsp_ratio`), not Parley inline boxes: Parley always
  allows a wrap after an inline box, which strands the spacer from its word.

## Next up

Font fallback for symbol glyphs (`✓` renders as a missing-glyph box: Parley
isn't picking a symbols font for it), then form controls / Sciter behaviors
(`Element.value`, text input, checkbox) which also give `state` flags real
meaning. Other open items: Tab focus order, element-level scrolling,
incremental restyle, `@env`/`@sciter` modules (README roadmap #2).
