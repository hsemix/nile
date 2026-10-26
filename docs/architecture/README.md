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
