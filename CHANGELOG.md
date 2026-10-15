# Changelog

All notable changes to Nile are recorded here, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/). Nile has no tagged release yet, so everything
sits under **Unreleased** and is grouped by the day it landed. Add a line under `Unreleased` in
the same change that makes it (Added / Changed / Fixed / Removed); when a version is tagged,
the section becomes that version's.

## Unreleased

### 2026-10-15

#### Added
- `prototype: Class url(file.js)`: CSS binds a script class (extending `Element`) to every matching element. The file is loaded as a module (an export named like the class, else the default export; or a class the page defines), each element becomes an instance (fields run), methods named `"on <event> [at <selector>]"` (`^event` = capture) become handlers (returning `true` marks the event handled), `componentDidMount()` runs and `componentWillUnmount()` runs on removal. Example `examples/prototype/`.
- Transparent windows: `window-frame="transparent"` now really shows the desktop through the page's transparent areas on macOS (straight alpha: softbuffer's macOS backend offers no premultiplied mode), and `window-material="blur|acrylic|mica|tabbed|sidebar|hud"` adds the system's frosted backdrop (macOS `NSVisualEffectView`, Windows 11 backdrop through winit, KDE blur). Examples `examples/transparent.html`, `examples/material.html`.
- C API: `NileWindowConfig` gains `translucent`, `shadow` and `material`; `nile_view_set_dialogs` (open/save/folder/message callbacks), `nile_view_drop_files`, `nile_view_window_activated`, `nile_view_portals` + `nile_view_render_portal` (popups in OS windows of their own) and optional `get_html`/`set_html` on `NileClipboard`. Appended fields: recompile hosts that build these structs.
- `window-frame="solid-with-shadow"` now draws the system's drop shadow (macOS `hasShadow`, Windows undecorated shadow); `solid` and `transparent` have none. C API: `frame` is 4 for it. Example `examples/solid-shadow.html`.
- `Window.this.material` (get/set) changes `window-material` while the window is open.
- Windows: translucent windows are drawn through a 32-bit DIB with an alpha mask (`alpha_win.rs`) because softbuffer's Win32 backend drops alpha; Wayland windows use softbuffer's ARGB buffers.
- `customElements.define(name, Class, {extends})` (plus `get`, `getName`, `whenDefined`, `upgrade`): the Web Components way to bind a class to a tag, on the same machinery as `prototype:`. Parsed, `createElement`ed (also `{is}`), `new MyElement()` and later-inserted elements become instances; `connectedCallback`, `disconnectedCallback`, `attributeChangedCallback` (`static observedAttributes`) run. `attachShadow()` returns the element itself (no encapsulation, no slots). Example `examples/custom-elements.html`.
- Queries started from an element accept selectors that begin with a combinator (`el.$(">strip>label")`) and read `:root` as the element itself (Sciter's shape); delegated `on()` handlers too.
- `:focusable`/`el.state.focusable` (makes any element focusable and tabbable), `:tab-focus` (focused by the Tab key), `:owns-focus` (a descendant is focused).
- Dynamic `import()` and module resolution accept `file://` and `this://` URLs (`__DIR__ + "x.js"`).
- `backface-visibility: hidden` (a flip card shows only the face turned toward you; hit testing agrees) and `transform-style` is parsed. Example `examples/flip-card.html`.
- `:link`, `:any-link` and `:visited` (following an `<a href>` marks it visited; `el.state.visited`); UA colour for visited links.
- `list-style-image` and the `list-style` shorthand's picture. Example `examples/list-style-image.html`.
- `font-variation-settings` and `font-feature-settings`; variable fonts are now drawn at the right axis positions (before, only the shaping used them, so `font-weight` and axes on e.g. the macOS system font looked wrong). Example `examples/font-variations.html`.
- `@sys`: `Pipe` (named pipes: Unix sockets; a loopback emulation on Windows), `TTY(fd)` and `fs.watch(path, cb)`. Test `the_sys_module_has_named_pipes_ttys_and_fs_watch`.
- Scrollbar styling: `scrollbar-width` (`auto`/`thin`/`none`), `scrollbar-color`, `overflow: scroll-indicator`, and `::-webkit-scrollbar`, `-thumb`, `-track` (thickness, colours, thumb rounding). The page's own bar follows `<html>` (else `<body>`). Example `examples/scrollbars.html`.
- Scrollbars react to the pointer: the bar under it grows, shows its track and darkens its thumb, darker again while dragged.
- Example `examples/combobox.html`: a filtering combobox written as a class component, with keyboard navigation and a popup list.
- Date, month, week, `datetime-local`, time and colour inputs open pickers (calendar, month grid, week-aware calendar, hour/minute columns, saturation/hue square with hex field and swatches) from a glyph at the field's edge (the swatch for colours) or Alt+Down; they set `value` and fire `input`/`change`. Example `examples/pickers.html`.
- Constraint validation: `validity`, `validationMessage`, `willValidate`, `checkValidity()`, `reportValidity()`, `setCustomValidity()` (form-level too) for `required`, `min`/`max`/`step`, `minlength`/`maxlength`, `pattern`, `email`, `url`; `:valid`/`:invalid` selectors.
- `type=number` steps with the wheel when focused and under the pointer.
- Files dropped on a window go to the file input under the pointer (first file only unless `multiple`), else a `drop` event with `dataTransfer.files`.
- `<legend>` sits on the fieldset's top border with a gap in the line.
- `Element.scrollIntoView()` scrolls scrollable ancestors, not only the page.
- Tables: `caption-side: bottom`, backgrounds/borders on `thead`/`tbody`/`tfoot` (an empty box behind their rows, also repeated with a paginated header), and automatic columns in a table with a `width` share the free space in proportion to their text instead of equally. Example `examples/table-parts.html`.
- Sciter class components: a class extending `Element` works as a JSX tag with `this(props, kids)`,
  `render()`, `componentDidMount`, `componentDidUpdate`, `componentWillUnmount`; `componentUpdate(obj)`
  re-renders in a batched microtask; `Element.create(jsx)`, `el.post(fn)`. Example `examples/components.html`.
- `Element.value` of a `<form>` (or `behavior="form"`) is an object of its named controls; assignable.
- `@debug` module (`setUnhandledExceptionHandler`, `setConsoleOutputHandler`, `callStackAt`; the
  breakpoint/inspector functions are stubs) and `@storage` / `Storage.open` (JSON file with
  `root`, `commit`, `close`, `registerClass`; no indexes or encryption).
- This changelog.

#### Changed
- `el.attributes["name"]` is the attribute's value as a string and assigning to it sets the attribute (Sciter's shape, as in `label.attributes["for"]`); it used to be an `{ name, value }` object (numeric indexes and iteration still give those).

#### Fixed
- Windows: closing a window (or resizing a translucent one) overflowed the stack: the alpha presenter's `release()` re-dropped itself. Fixed.
- Windows CI: `tty_size` no longer calls `unwrap_or` on a constant `None` (clippy).
- An `html`/`body` background with alpha was drawn opaque; a translucent window keeps the alpha, an ordinary window shows it over white.
- Sibling selectors and `:nth-child` on very wide lists no longer scan the child list per element (a per-version sibling index in `Document`).
- The pointer is an arrow, not a text cursor, over a number input's spin arrows and a date/time/colour input's picker glyph.
- Setting `input.value` from a script no longer makes the engine fire a second `change` when the input loses focus.
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
