# Architecture notes

How Nile is put together, for contributors. The [README](../../README.md) says what Nile does and the [guide](../guide/README.md) how to use it; these notes say how the engine does it and what to watch for when changing it.

Data flows one way: `nile-dom` -> `nile-style` -> `nile-layout` (uses `nile-text`) -> `nile-render`, with `nile-script` and `nile-engine` on top and `nile-shell` / `nile-capi` around them.

| Topic | Covers |
|---|---|
| [Core invariants and engine plumbing](architecture-core.md) | The document, layout and script bridges, the C libraries, and the small rules that keep rendering correct. |
| [Style, cascade and values](style.md) | Parsing, the cascade, restyling, themes, fonts, scrollbars and the modern CSS mechanisms. |
| [Layout and painting order](layout.md) | Scrolling boxes, tables, lists, floats, positioning, stacking, transforms and pagination. |
| [Forms, selection and editing](forms-and-editing.md) | Form controls, text and page selection, rich-text editing, the clipboard, IME, pickers and drag and drop. |
| [Scripting runtime and libraries](scripting.md) | The JS platform in `nile-script`, `@nile/*` libraries, modules, components, crypto, printing, mail, serial, hotkeys and similar. |
| [Canvas, SVG and audio](graphics-and-media.md) | `<canvas>`, SVG rendering and `<audio>`. |
| [Windows and the native shell](windows-and-shell.md) | Window configuration, frames, multiple and owned windows, popups, dialogs, menus, tooltips, translucent windows and platform chrome. |
| [Accessibility](accessibility.md) | The accessibility tree, text runs and live regions. |
| [Packaging, updates, lifecycle and native extensions](packaging-and-extensions.md) | `--pack`, signed updates, single-instance and file/URL events, and the native extension host. |

## Golden images

`crates/nile-engine/tests/golden.rs` paints text-free pages (boxes, borders, gradients, flex and grid, stacking,
transforms, filters, clipping, floats, tables, canvas, SVG) and compares them with the PNGs in
`crates/nile-engine/tests/golden/` (a few levels of per-channel tolerance, 0.2% of pixels may differ). Text is left out
because every system has its own fonts. To change a page or accept a deliberate visual change, run
`NILE_UPDATE_GOLDEN=1 cargo test -p nile-engine --test golden` and **look at the new PNGs** before committing them: a
golden that encodes a bug will keep the bug. A failing comparison writes `<name>.actual.png` and `<name>.diff.png` (changed
pixels in red) to `target/golden-failures/`, which CI uploads as an artifact. Known gap: `conic-gradient` isn't drawn, so
no page uses it.

## Manual testing

[`docs/testing/platform-checklist.md`](../testing/platform-checklist.md) is the hands-on checklist for Windows and Linux (menus, tray, printing, credential stores, hotkeys, serial, camera, packaging, screen readers): CI only proves those paths compile.
