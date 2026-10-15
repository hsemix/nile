# Changelog

All notable changes to Nile are recorded here, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/). Nile has no tagged release yet, so everything
sits under **Unreleased** and is grouped by the day it landed. Add a line under `Unreleased` in
the same change that makes it (Added / Changed / Fixed / Removed); when a version is tagged,
the section becomes that version's.

## Unreleased

### 2026-10-15

#### Added
- Sciter class components: a class extending `Element` works as a JSX tag with `this(props, kids)`,
  `render()`, `componentDidMount`, `componentDidUpdate`, `componentWillUnmount`; `componentUpdate(obj)`
  re-renders in a batched microtask; `Element.create(jsx)`, `el.post(fn)`. Example `examples/components.html`.
- `Element.value` of a `<form>` (or `behavior="form"`) is an object of its named controls; assignable.
- `@debug` module (`setUnhandledExceptionHandler`, `setConsoleOutputHandler`, `callStackAt`; the
  breakpoint/inspector functions are stubs) and `@storage` / `Storage.open` (JSON file with
  `root`, `commit`, `close`, `registerClass`; no indexes or encryption).
- This changelog.

#### Fixed
- Linux CI: `a_registered_font_is_found_under_its_family_name` compared widths against a fallback
  that is the same font on Linux.

### 2026-10-14

#### Added
- Release builds carry the Nile icon: `assets/icon.html` (drawn in CSS, rendered by Nile) -> `icon.png`;
  built-in default window icon; `--pack` without `--icon` gives bundles/.exe the default; `nile --brand`
  stamps icon and version info into a Windows exe; release archives include `nile.png` (+ `nile.desktop` on Linux).
- `font` shorthand (`expand_font`).
- `@font-face` (TTF/OTF/WOFF/WOFF2 from files, `data:` URLs and the packed app) and `@supports`.
- `Blob`, `File`, `FormData` (multipart), streams (`ReadableStream`, `WritableStream`, `TransformStream`,
  text encoder/decoder streams) and `WebSocket` (ws and wss over rustls).
- `@sys` module: processes, pipes, TCP/UDP sockets, file handles, environment helpers.
- `<link rel=stylesheet>` and `@import` of style sheet files.
- Packaging details: `--name --version --description --company --copyright --category --install`,
  Windows version resource, `.desktop` entry on Linux, window icon from a packed `.png`.
- Platform themes (Windows, macOS, Linux look for controls); `NILE_THEME` override.
- Native Windows common dialogs (open/save/folder/message) and `osascript`/`zenity`/`kdialog` elsewhere.
- Text around floats and `shape-outside`; `<tfoot>` repeated on paginated tables.
- Accessibility: text runs, selections, live regions.
- Frames follow transforms, scrolling and clipping of their containers.
- Window-request API in the C ABI (`nile_view_take_window_request`, `nile_view_create_window`, ...).
- Partial restyle for hover/active/focus (about 15 ms -> 0.2 ms per pointer move on the 1,500-row page);
  `transition-property` shorthands.
- Examples: file explorer, virtual data grid, Northstar POS, font-face, shape-outside, css-file.

#### Changed
- `overflow` clipping follows `border-radius` when painting.
- `font-family` keeps the whole list so a missing family falls through to the next.

#### Fixed
- Windows: handle types (`isize`), menu pointers, dialogs opening behind the window, taller-than-expected
  tests under Segoe UI.
- CI logs show errors (`CARGO_TERM_COLOR: never`).
- Table header repeating on paginated tables; `-o Name.app` collision when packing.

### 2026-10-12 to 2026-10-14 (first builds)

#### Added
- Engine: HTML5 parsing into an arena DOM, CSS cascade (Servo `selectors`), Taffy layout (flex, grid,
  tables, floats, positioned/fixed/sticky, stacking contexts), Parley text, Skia painting, QuickJS scripting
  with JSX, modules, a DOM/web-platform layer and Sciter helpers.
- Controls and editing: form controls, `<select>`, text selection and clipboard, rich-text editing with
  tables, images, undo, IME.
- Windows and shell: winit window with frameless/role-based chrome, multiple windows, modal dialogs,
  popups in their own OS windows, native menu bar (macOS, Windows), tooltips.
- `<include>`, `<frame>`/`<iframe>`/`<frameset>`, accessibility tree (AccessKit), pagination, paged
  view and PDF output, single-file packaging with a start file, C ABI (`libnile`, `nile.h`).
- CI and release workflows for macOS, Windows and Linux.
