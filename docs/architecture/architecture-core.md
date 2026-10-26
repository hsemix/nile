# Core invariants and engine plumbing

[Architecture index](README.md)

Implementation notes for people changing Nile: The document, layout and script bridges, the C libraries, and the small rules that keep rendering correct. Each note says where the code is and which trap it avoids. They are kept as written while the code was built, so check a name against the source before relying on it.

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
- Embedding: everything that isn't winit lives in `nile-engine` (`Page` is the entry point, with `pub` fields/methods for the shell and C API; keep winit and arboard out of it, `system-clipboard` is an optional feature). `nile-capi` wraps `Page` in `NileView` with `extern "C"` functions, each guarded by `catch_unwind`; adding one regenerates `include/nile.h` on build (commit it). Scripts get `host.name(...)` (JSON in/out, registered with `ScriptHost::register_host_fn`), the host gets `eval`/`call`. `examples/c/render.c` is the smoke test; build it when you change the API.
- C API additions: `NileWindowConfig::{translucent, shadow, material}`, `NileDialogs` -> `CDialogs` (a `Dialogs` impl over blocking callbacks), `NileClipboard::{get_html, set_html}`, `nile_view_drop_files`, `nile_view_window_activated`, `nile_view_portals`/`nile_view_render_portal` (JSON list + `paint_portal`; the host adds the rectangle's origin to pointer input). Struct fields were appended, so C hosts must recompile.
- Two C libraries: `libnile` (nile-capi, embedding only, no winit) and `libnile_app` (nile-app: `pub use nile::*` re-exports every `nile_*` symbol, plus `nile_run`/`nile_run_file`/`nile_run_html`/`nile_app_register_function`; header `nile_app.h` includes `nile.h`). The shell is `nile_shell::run(argv) -> exit code` (the `nile` binary is a 6-line `main`); `nile_script::add_default_host_fn` is a thread-local registry every `ScriptHost::new` starts from, so host functions exist before a page's first script. Must run on the main thread. The release workflow packages both.
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

