# Changelog

All notable changes to Nile are recorded here, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/). Nile has no tagged release yet, so everything
sits under **Unreleased** and is grouped by the day it landed. Add a line under `Unreleased` in
the same change that makes it (Added / Changed / Fixed / Removed); when a version is tagged,
the section becomes that version's.

## Unreleased

### 2026-10-18

#### Fixed
- Horizontal `margin` on inline elements (`<span style="margin-right: 20px">`) was ignored; it now takes space. Large inline padding or margin no longer makes the line taller (the spacer run is a one-pixel space stretched with letter spacing instead of a huge font).

#### Added
- CSS text properties (`examples/css-text.html`): `text-indent` (positive, first line), `word-spacing`, `word-break` (`break-all`, `keep-all`; `break-word` as `overflow-wrap: anywhere`), `overflow-wrap`/`word-wrap`, `text-shadow` (several shadows, blur), `text-decoration-style` (`double`, `dotted`, `dashed`, `wavy`), `text-decoration-color`, `text-decoration-thickness`, `text-underline-offset` and the full `text-decoration` shorthand, `-webkit-line-clamp`/`line-clamp` (with `display: -webkit-box`; the last line ends in an ellipsis), `text-wrap: balance` (two to six lines), `caret-color` and `font-variant` / `font-variant-numeric` (OpenType features through `font-feature-settings`).
- CSS flow and box properties (`examples/css-flow.html`): `visibility` (inherited; `hidden`/`collapse` keep the space, draw nothing of the box's own and aren't hit, a child can set `visible` again), `pointer-events: none` (inherited; hits pass through to what is below), `outline`, `outline-width/-style/-color/-offset` (drawn outside the border box, follows `border-radius`, takes no space), `order` on flex and grid items, the `flex-flow` shorthand (and `wrap-reverse` is read as `wrap`) and `aspect-ratio` (`2`, `16 / 9`, `auto`; also on images).

### 2026-10-17

#### Added
- SVG: inline `<svg>` (sized by `width`/`height` or `viewBox`, `currentColor` follows the CSS colour, page style rules reach its elements, `createElementNS`/`setAttributeNS`), `<img src="*.svg">`, `data:image/svg+xml` and `background-image: url(*.svg)`, drawn with resvg (new crate `nile-svg`) at the size and pixel density they are shown at. SVG `<text>` is a feature (`svg-text` on `nile-engine`, on by default, about 0.7 MB); the rest of SVG adds about 0.8 MB. Guide `docs/guide/svg.md`, example `examples/svg.html`.
- `<canvas>` with the 2D context (new crate `nile-canvas`, Skia raster surfaces): paths, arcs, `Path2D` (including SVG path strings), gradients, patterns, shadows, line dashes, compositing modes, transforms, clipping, `fillText`/`strokeText`/`measureText`, `drawImage` (images and canvases), `getImageData`/`putImageData`, `toDataURL`/`toBlob`, `OffscreenCanvas`; drawing repaints without a new layout. `<img>` gets `naturalWidth`/`naturalHeight`/`complete`, `src` load/error events and `new Image()`. Guide `docs/guide/canvas.md`, example `examples/canvas.html`.
- `MediaQueryList` change events: a list that has a `change` listener, an `onchange` or an `addListener` callback is re-checked when the window resizes or the system theme flips, and fires a `MediaQueryListEvent` (`matches`, `media`) when its answer changed.
- `color-scheme` on any element: its controls take the dark or light user-agent look and its system colours (`Canvas`, `ButtonFace`...) resolve in that scheme, whatever the page or the system uses (`examples/color-scheme.html`). `color-scheme: dark` on the root now makes the page dark on a light system too (it used to need a dark system); `light dark` still follows the system.
- Native extensions: a small C ABI (`nile-extension-sdk`, header `nile_extension.h`) lets a separately distributed shared library add namespaced operations (immediate or asynchronous, JSON plus binary buffers), managed resources, event subscriptions and cancellation requests to a page. `import sqlite from "@sqlite"` loads a package (`extension.json` + `index.js` + `native/<platform>/<library>`) declared in the application's `nile.json`; completions wake the event loop and settle Promises on the UI thread. SQLite (`extensions/sqlite`, bundled `rusqlite`, one ordered worker per context) and a notifications event source are the two independent consumers; neither is linked into the engine. Packed apps (`nile --pack`) extract only the native files to a private, hash-verified cache and keep wrappers in the archive; stable writable data lives under `application.dataPath`. Guide: `docs/extensions.md`, example `examples/sqlite/`. Extensions must unwind to catch their own panics, so the workspace has an `extension` profile (release + `panic = "unwind"`, used by `cargo run -p nile-extensions --example build-extension -- sqlite --release --dest dist/sqlite`); the engine keeps `panic = "abort"`.
- `examples/register.html`, `examples/admin-registration.html` and `examples/prototype/serial-number.js`: a `prototype:` class that formats a product key while keeping native selection and clipboard.

#### Fixed
- The release binary grew by about 1.2 MB when the workspace release profile switched to `panic = "unwind"` for extensions. It is back to `panic = "abort"`; only extension builds unwind (see the `extension` profile above).
- An extension reply over the 1 MiB limit now rejects the call with `RESULT_LIMIT` instead of leaving its Promise pending when the extension ignores the `NILE_OVERLOADED` it gets back.
- Packed extensions failed at startup when the user's cache or data root sat behind a symlink (a symlinked `/home` on Fedora Atomic, a dotfile-managed `~/.cache`). Links in the OS-provided root are followed now; links below it are still refused.
- The search for `nile.json` no longer climbs to the filesystem root or into the user's home folder (or above it), so a stray file there cannot load native code into every page below.
- A native extension whose `nile_extension_init` failed no longer gets its `context_dispose`/`shutdown` hooks called; one whose `context_init` failed gets `shutdown` only.
- `nile_extension.h` declares `nile_extension_init` with a `NILE_EXTENSION_EXPORT` macro (`__declspec(dllexport)` on Windows, default visibility elsewhere), so a C extension that defines it with `dllexport` no longer fails to compile under MSVC (C2375).
- SQLite's `dispatch` answers `NILE_INVALID` (not `NILE_CLOSED`) to a subscribe request, which it doesn't support.

### 2026-10-15

#### Added
- `nile-password-char: "*"`: the developer picks the character a password input shows for each typed one (any single character; default `•`); the caret, selection and IME follow its width. Example `examples/password-char.html`.
- `prototype: Class url(file.js)`: CSS binds a script class (extending `Element`) to every matching element. The file is loaded as a module (an export named like the class, else the default export; or a class the page defines), each element becomes an instance (fields run), methods named `"on <event> [at <selector>]"` (`^event` = capture) become handlers (returning `true` marks the event handled), `componentDidMount()` runs and `componentWillUnmount()` runs on removal. Example `examples/prototype/`.
- Transparent windows: `window-frame="transparent"` now really shows the desktop through the page's transparent areas on macOS (straight alpha: softbuffer's macOS backend offers no premultiplied mode), and `window-material="blur|acrylic|mica|tabbed|sidebar|hud"` adds the system's frosted backdrop (macOS `NSVisualEffectView`, Windows 11 backdrop through winit, KDE blur). Examples `examples/transparent.html`, `examples/material.html`.
- `libnile_app` (new crate `nile-app`): libnile plus a standalone runner for C programs that don't want to write window code: `nile_run(argc, argv)` (the `nile` program's own command line), `nile_run_file`, `nile_run_html` and `nile_app_register_function` (`host.name(...)` from the first script on). Header `nile_app.h`; example `examples/c/app.c`; the release builds ship both libraries. `nile-shell` is now a library (`nile_shell::run`, `run_page`) with a thin `nile` binary, and `nile_script::add_default_host_fn` registers host functions for every page.
- C API: `NileWindowConfig` gains `translucent`, `shadow` and `material`; `nile_view_set_dialogs` (open/save/folder/message callbacks), `nile_view_drop_files`, `nile_view_window_activated`, `nile_view_portals` + `nile_view_render_portal` (popups in OS windows of their own) and optional `get_html`/`set_html` on `NileClipboard`. Appended fields: recompile hosts that build these structs.
- `window-frame="solid-with-shadow"` now draws the system's drop shadow (macOS `hasShadow`, Windows undecorated shadow); `solid` and `transparent` have none. C API: `frame` is 4 for it. Example `examples/solid-shadow.html`.
- Windows: the symbol blocks from Arrows through Miscellaneous Symbols and Arrows (U+2190-2BFF) use Segoe UI Symbol explicitly, because the system fallback there found no font for several of them (boxes under plain `sans-serif`).
- Scrollbars follow the platform: macOS overlay bars are a little thicker (7px, 5px thin, +3px on hover), Windows 11's start as a thin line (4px, 3px thin) that widens by 6px under the pointer, Linux keeps 6px/4px/+4px; in a dark page the thumb is light. `nile_style::{scrollbar_auto, scrollbar_thin, scrollbar_grow}`. `::-webkit-scrollbar`/`scrollbar-width`/`scrollbar-color` still override.
- System fonts: `system-ui` (and the aliases pages put first in their stacks: `-apple-system`, `BlinkMacSystemFont`, `ui-sans-serif`) is the platform's UI font (San Francisco, Segoe UI Variable/Segoe UI, Cantarell/Noto Sans), `ui-monospace`, `ui-serif` and `ui-rounded` map to the platform's monospace, serif and rounded fonts, and `font: menu|caption|icon|message-box|small-caption|status-bar` (and Sciter's `font: system`) give the UI font at the platform's size. The default font of pages and controls is now `system-ui` on all three platforms (it was `sans-serif` on macOS and Linux). The system fonts get their `opsz` (optical size) axis set to the text size like browsers do; without it San Francisco came out tight and spindly. Examples `examples/system-fonts.html`.
- Controls have hover, pressed and keyboard-focus states in every platform theme (light and dark): buttons, selects, file/button inputs lighten on hover and darken when pressed, text fields and checkboxes react to hover, and `:focus-visible` (new: focused by the keyboard, or a control you type into) draws the platform's focus ring on buttons, selects, checkboxes and radios, so a mouse click no longer shows it. Every page now restyles on hover through the cheap partial restyle.
- Dark mode and system colours. The `nile` shell reads the OS's light/dark setting (winit, live through `ThemeChanged`) and accent colour (macOS `NSColor.controlAccentColor`, Windows registry `AccentColor`, GNOME `gsettings`), and `nile_style::set_system_theme` hands them to the style engine (C hosts: `nile_view_set_system_theme`). `@media (prefers-color-scheme: dark)` and `matchMedia` are real now; the system colour keywords (`Canvas`, `CanvasText`, `ButtonFace`, `ButtonText`, `ButtonBorder`, `Field`, `FieldText`, `GrayText`, `LinkText`, `VisitedText`, `Highlight`, `HighlightText`, `AccentColor`, `AccentColorText`, `Mark`) follow the theme; `color-scheme: light dark` (on the root, or `<meta name="color-scheme">`) makes a page take the dark look on a dark system (canvas, text, links, buttons, inputs, selects, checkboxes, popups, pickers, message boxes) with per-platform dark control sheets; `accent-color` is supported. The system accent colours checked boxes and radios, focus rings, menu highlights, progress bars, ranges and text selection. `NILE_COLOR_SCHEME=dark|light` and `NILE_ACCENT=#rrggbb` override them (screenshots, tests). Example `examples/dark-mode.html`.
- Windows 11 Snap Layouts: in a frameless window (`solid`, `solid-with-shadow`, `transparent`) the element with `role="window-maximize"` is reported to the system as the maximize button (`WM_NCHITTEST` -> `HTMAXBUTTON`), so resting the pointer on it shows the snap flyout. Hover is fed back to the page, and the click maximizes or restores through the system (the element gets no `click` event then). `Page::maximize_button_rect` for embedders. Example `examples/snap-layouts.html`.
- Symbols: the marked and fill-pattern squares (`▣ ▤ ▥ ▦ ▧ ▯`, U+25A2-25AF), the arrows from U+21B0 (`↶ ↷ ⇄`) and Miscellaneous Technical (`⌕`, U+2300-23FF) now get the named symbol fallback fonts, so they no longer show as empty boxes under plain `sans-serif` (the system fallback has no font for them on macOS). `examples/symbols.html` shows each symbol in several font stacks, for finding which setup lacks a glyph.
- Collapsed table borders resolve conflicts: on an edge two cells share, the wider border wins, then solid over dashed over dotted, then the upper/left cell; the other cell leaves that side out instead of painting over it. Example `examples/table-collapse.html`.
- Rounded windows: a frameless window (`solid`, `solid-with-shadow`, `transparent`) is clipped to the root `<html>`'s `border-radius`, so `window-material` takes the page's rounded shape instead of showing a rectangle around it (Windows: a window region, not anti-aliased and without the system shadow; macOS: layer corner radius on the page and effect views). Square again when maximized or full screen. On Windows `window-material="sidebar"` is now Mica (acrylic is `acrylic`, `hud` and `blur`). `examples/material.html` rounds its window.
- Windows 11: windows with a system frame (`window-frame` default/`standard`/`extended`) get Mica in the title bar by default, so it picks up the wallpaper/accent tint like File Explorer and Sciter windows instead of staying plain white. `window-material="none"` keeps the plain one (an unset or unknown value is `auto`). Older Windows keeps the plain bar.
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
- macOS: opening a child, tool or modal window (`new Window`, `modal`) panicked with "The AccessKit winit adapter must be created before the window is shown": attaching the window to its owner ordered it in before the adapter existed. It is attached afterwards now.
- `<legend>` shrinks to its label, so a fieldset's top border is only cut behind the label instead of across the whole width (the user-agent sheet's `width: fit-content` was ignored). `width: fit-content` / `max-content` / `min-content` now work on block, flex and list-item boxes: the box takes its max-content width, never wider than its container.
- The pointer is an arrow, not a text cursor, over `<input type=range>`, `<progress>`, `<meter>`, `<input type=file>` and reset buttons.
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
