# Changelog

All notable changes to Nile are recorded here, newest first, in the spirit of
[Keep a Changelog](https://keepachangelog.com/). Nile has no tagged release yet, so everything
sits under **Unreleased** and is grouped by the day it landed. Add a line under `Unreleased` in
the same change that makes it (Added / Changed / Fixed / Removed); when a version is tagged,
the section becomes that version's.

## Unreleased

### 2026-10-27

#### Added
- `application.registerHotkey("CmdOrCtrl+Shift+K", handler)`: system-wide hotkeys that work while the app is in the background (`global-hotkey` crate). Returns `{ ready, close() }`; `ready` says whether the system accepted the key. `examples/hotkeys.html`, [guide](docs/guide/app-lifecycle.md). Linux: X11 only.

### 2026-10-26

#### Added
- `@nile/mail`: send email over SMTP (STARTTLS or TLS, AUTH PLAIN/LOGIN, HTML and text, attachments, inline images, RFC 2047 headers, dot-stuffing, header-injection refusal) with `buildMessage` for the raw message. `examples/mail.html`, [guide](docs/guide/email.md).
- `@nile/keychain`: secrets in the system credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service) with `set`/`get`/`remove`, binary `setBytes`/`getBytes`, `available()`; Promises on a worker thread; cargo feature `keychain` (default in nile-engine; `keyring` crate). [guide](docs/guide/credentials.md).
- `fs.watch` uses real file notifications (inotify, FSEvents, ReadDirectoryChangesW via the `notify` crate; cargo feature `watch`, default in nile-engine) instead of polling every 250 ms; bursts arrive together, and it still polls when the feature is off or the system refuses a watch.

#### Fixed
- `width: 100%` on an `<input>`, `<select>` or other inline-block was ignored (layout sizes inline boxes on their own, where a percentage had nothing to refer to), so form fields stayed content-sized; a full-width inline-block now fills its container like in a browser. Smaller percentages still size to the content.

### 2026-10-25

#### Added
- `Intl` (`NumberFormat`, `DateTimeFormat`, `PluralRules`, `ListFormat`, `RelativeTimeFormat`, `Collator`) and working `Number/Date.prototype.toLocale*String` / `String.prototype.localeCompare` (numeric and accent-aware sorting) for about two dozen locales, written in JS over a small data table instead of ICU. `navigator.language` is now the OS language and region instead of a fixed `en-US`. [guide](docs/guide/internationalization.md).
- `@env` `reveal(path)` (show in Finder/Explorer), `moveToTrash(path)` (Trash/Recycle Bin, file or folder) and `openWith(path, app)`. [guide](docs/guide/desktop-helpers.md).

#### Fixed
- `fs.realpath` (`@sys`) no longer returns Windows' verbatim `\\?\C:\...` form; the prefix is dropped (`\\?\UNC\server\share` becomes `\\server\share`).

### 2026-10-24

#### Added
- Crypto: the standard `crypto.getRandomValues`, `crypto.randomUUID` and `crypto.subtle` (digest SHA-1/256/384/512, HMAC, PBKDF2, AES-GCM; raw keys) backed by `ring`, with the heavy calls on a worker thread, and `@nile/crypto` (`hashPassword`/`verifyPassword`/`needsRehash` with PBKDF2-SHA256 at 600,000 iterations, `sha256`, `hmac`, `encryptText`/`decryptText`). Checked against SHA-2, HMAC, PBKDF2 and NIST AES-GCM vectors. [guide](docs/guide/hashing-and-encryption.md).
- Printing: `Window.this.print({ printer, copies, title })` and `Window.this.exportPdf(path)` write the page as a PDF (the `@page` size and margins, page breaks, repeated table headers) and send it to the system printer or save it, without touching the on-screen layout; `@nile/print` (`printers()`, `printFile`, `printRaw`, `sendNetwork`: CUPS `lp` on macOS/Linux, PowerShell and winspool on Windows, raw sockets); `@nile/escpos` (`Receipt` builder for ESC/POS thermal printers: text, rows and columns, size/bold/underline, barcodes, QR, logo images, cut, cash drawer, code pages 437/858/1252, text preview; `printReceipt` to an installed printer, a network printer or a device path). `examples/printing.html`, [guide](docs/guide/printing.md).
- Six more `@nile/ui` components, each with an example page and tests: `<nile-tree>` (expand/collapse, lazy children, filter, keyboard and type-ahead), `<nile-keypad>` (number pad for a field or a PIN, decimal/sign/quick amounts), `<nile-calendar>`, `<nile-date>` and `<nile-date-range>` (typed or clicked dates in any format, min/max, two-month range picker with presets), `menu()`/`contextMenu()` (popup menus with submenus, check marks, shortcuts), `palette` (a Ctrl+K command palette with fuzzy matching that also lists labelled shortcuts, and a general picker) and `<nile-wizard>` (steps with sync or async validation, optional and skipped steps). `examples/ui-tree.html`, `ui-inputs.html`, `ui-commands.html`, `ui-wizard.html`, [guide](docs/guide/ui-components.md).
- `examples/jsx-components.html`: new tags and inputs written as classes and JSX (a rating input, a quantity stepper, a class component used as a JSX tag).

#### Fixed
- Removing the focused element (a closed menu or dialog) leaves nothing focused, so keys reach the document and its shortcuts again; they used to go to the detached element.
- `display: inline-flex` and `inline-grid` are atomic inline boxes that shrink to fit and sit in a line, as `inline-block` does; they used to behave as full-width block flex/grid containers. `getComputedStyle` reports the inline value.

### 2026-10-23

#### Added
- `<nile-grid>` cell editing (`editable` columns; F2/Enter/double-click/typing, Enter/Tab/Escape, text, number, select and check-box editors, `validate`, `parse`, `set`, `cellchange` events, computed columns refreshed) and keyboard column resizing (focusable headers, Shift+Left/Right, Enter to sort).
- More `@nile/ui` components, each with an example page and tests: promise **dialogs** (`confirm`, `prompt`, `alert`, `dialog`, `<nile-dialog>`: focus trap, Escape/Enter, validation), **toasts**, **shortcuts** (`shortcuts.on("Mod+S", fn)`, `data-shortcut` buttons, quiet while a dialog is open), **tabs** and **accordion**, a **combobox** (filtering, groups, hints, async `source`, `allow-custom`), a restylable Microsoft-style **ribbon** (`ribbon(config)`: tabs, groups, large/small/stack/row items, toggles, selects, drop-down and split menus, contextual tabs, quick-access bar, collapse), a **tile grid** for F-key pads and launchers, and **charts** (`<nile-chart>`: bar, line, stacked area, pie, donut, tooltips, legend toggling). `examples/ui-dialogs.html`, `ui-navigation.html`, `ui-combobox.html`, `ui-ribbon.html`, `ui-charts.html`, [guide](docs/guide/ui-components.md).
- Sciter's layout CSS: `flow: horizontal|vertical|horizontal-wrap|vertical-wrap` (also `row`/`column`), `*` and `N*` lengths (`width: *`, `width: 2*`, `height: *`, `margin-left: *`: shares of the leftover space along a flow, stretch across it, an auto margin), the `size: <w> [<h>]` shorthand and the `dip` unit. Needed by the soft-school app port. `examples/sciter-flow.html`.
- Sciter's `flow: grid(1 2 3, 4 4 4)` (cells numbered by child position, repeated numbers span) and Sciter's HTML tag shortcuts: `<div #id>`, `<tag .class>`, `<input (name)>`, `<button|radio(group).first>` (`|type`, `#id`, `.class`, `(name)`, glued to the tag or standing alone), which sit beside the existing `<menu.context>`.
- Sciter's CSS spellings and chrome: `var(name): value` / `var(name)` / `color(name)` / `length(name)` (rewritten to custom properties), `:theme(dark)` (matches the `theme` attribute), `url(path:M0 0 L10 10 ...)` SVG-path images drawn with `fill`, `stroke` and `stroke-width` (else the text colour), `foreground-image`/`-size`/`-position`/`-repeat` (painted over the content), the window constants `window-caption-height`, `window-button-width`/`-height`, `window-frame-width` and `window-accent-color`, and the system colours `window`, `windowtext`, `threedface`. An extended-frame page styled with Sciter's `win-frame.css` now draws its caption bar and minimize/maximize/close icons. `margin-left: *` on a button pushes it to the right edge. `examples/sciter-paths.html`.
- Sciter document quirks: `<!ENTITY name "value">` declarations with `&name;` references (icon-font entity files), `<style src="a.css" />` (self-closed too; the HTML parser used to swallow the rest of the page), markup between `<html>` and an explicit `<body>` (an extended window's `<header>` caption, a top bar) stays a child of `<html>` instead of being pulled into an implicit body, `html` then stacks it above a `body` that fills the rest of the window, and `height: *` works in ordinary block flow (the block becomes a column).
- `width: *` on an inline-block (a button) in ordinary flow fills its container, margins and padding included.

#### Fixed
- `<nile-grid>`: an open editor belongs to its record. Sorting, adding, removing or refreshing no longer writes the value to whichever record now sits at the old position, a refused (invalid) value keeps its editor through a refresh or a scroll instead of losing it, and a record that moves is scrolled into view. A column drag now also ends when the window loses focus or the grid is removed, reporting `columnresize` once.
- Author style sheets compete as one set in the cascade: a later `<style>` with a weaker selector (a component's `:where(...)` rule) no longer beats a stronger rule from an earlier sheet; equal strength still follows source order.
- Custom elements: properties assigned before an element is upgraded (e.g. `grid.rows = ...` right after `innerHTML`) no longer hide the class's setters; they are replayed through them on upgrade.
- The combobox and ribbon drop-down arrows are inline SVG chevrons, not the `▾` glyph some fonts lack.
- `@nile/ui` combobox made through `innerHTML` lost its initial `value` attribute (the element is upgraded after its attributes are set; the choice is now read when it is built).
- Styled `<select>`s: the open list now follows the select's `background`, border colour, `border-radius` and `accent-color` (and an `option`'s own `background`), where it was always drawn in the platform colours (white text on a white list for a dark select); a select keeps room for its chevron whatever `padding` the page sets and centres its label in a tall box. `examples/select-styles.html`.
- `<nile-grid>` column dragging does one style write per frame instead of one per row per mouse move (the widths are now two custom properties on the grid): 6.3 ms to 2.8 ms per move on the 100,000-row page. `bench_grid_resize` (ignored test) measures it.
- `url(...)` in a linked or `@import`ed style sheet is now relative to the sheet, not the page (a `@font-face` or background image next to the CSS file used to be missed).
- A `<button>` with `display: flex` or `grid` now lays out its own children (it wrapped them all in one centred column, so icon-and-label buttons stacked), and block/flex buttons shrink to their content like in browsers instead of filling the line.

### 2026-10-22

#### Added
- `@nile/ui`, an optional component library written in JavaScript and built into the program (`import ... from "@nile/ui"`; nothing to install, and a module that isn't imported costs nothing). First module: **barcodes and QR codes**: `<nile-barcode type value height module-width margin color background text ecc>` and `svg(type, value, options)`, `encode`, `dataUrl` for Code 128, Code 39, EAN-13, EAN-8, UPC-A and QR (byte mode, versions 1-40, error correction L/M/Q/H). Checked against `qrcode` (identical matrices for all 160 version/level combinations), `python-barcode` and OpenCV's readers. `examples/barcodes.html`, `docs/guide/ui-components.md`.
- `@nile/ui` drawer, splitter and data grid, each with an example page and tests: `<nile-drawer>` (slides in from any edge; backdrop, Escape and click-outside close, a cancelable `beforeclose`, focus kept inside and restored, `data-drawer-toggle|open|close` buttons that need no script), `<nile-splitter>` (draggable grip with a three-dot handle, keyboard resizing, min/max, click-to-collapse panes that restore their size, nesting, sizes remembered with `persist`, and the same size units as `<frameset>`: px, `%` and `N*` shares, per pane or as a `cols`/`rows` list on the host) and `<nile-grid>` with `<nile-column>` (draws only the visible rows so 100,000 cost the same as 30; columns with computed, formatted and custom cells; natural-order sorting, single/multiple selection with Shift/Ctrl, arrow/Page/Home/End keys, resizable columns, an empty state; events `selectionchange`, `rowactivate`, `sort`, `columnresize`). `examples/drawer.html`, `splitter.html`, `data-grid.html`, [guide](docs/guide/ui-components.md).
- Mouse events (`click`, `mousedown`, `mouseup`, `mousemove`, ...) now carry `shiftKey`, `ctrlKey`, `altKey` and `metaKey`; they were always false before, so Shift-click and Ctrl-click handlers couldn't work.

#### Fixed
- `<nile-splitter>` dragging: the panes are measured once when the drag starts, a pointer move that doesn't change the clamped size writes nothing, and `splitterchange` is sent once per frame (and when the drag ends) instead of on every move: 2.1 ms to 0.9 ms per move. `bench_splitter_drag` (ignored test) measures it.
- `<nile-grid>` scrolling: row elements are now reused instead of rebuilt when rows scroll in and out of view, the spacer is only rewritten when its size changes, and `selected`/`selectedIndex` no longer scan every row (`selectedCount` and `mountedCount` are new and O(1)). A frame while scrolling 100,000 rows now takes about 2 ms (6-10 ms on the frames that bring new rows in), down from 12-17 ms. `bench_grid_scroll` (ignored test; `NILE_BENCH_FILE` picks another page) measures it.
- `flex: 0 0 100px`, `flex: 0 0 auto` and any other `flex` shorthand with a bare `0` for `flex-shrink` ignored the basis (the `0` was read as `flex-basis: 0`); `flex: 1 200px` and `flex: 100px` now parse too.
- The `background` shorthand now resets the colour like the spec says: `background: none` and `background: url(...)` left an inherited or user-agent background colour (a button's, for one) showing.


### 2026-10-20

#### Added
- Dragging files out of the window: `dataTransfer.setFiles([...])` (or `file://` entries of `text/uri-list`) in `dragstart`; when the pointer leaves the window the page's drag ends (`dragend`, `dropEffect: "copy"`) and the system's own drag starts with the files (macOS `NSDraggingSession`, Windows `DoDragDrop` over a shell data object; written without a Windows machine). Not on Linux. `examples/drag-out.html`.
- `application.progress` (0 to 1, `"indeterminate"` or `null`): a progress bar on the dock icon (macOS, drawn in the dock tile) or the taskbar button (Windows, `ITaskbarList3`, written without a Windows machine). Not on Linux. In `examples/tray.html`.
- `window-remember` on `<html>` (a Nile extension): the window reopens with the size, position and maximized state it was closed with, saved per app (`nile.json` id, else the program's name) and per name in `<user data>/nile/windows/<app>.json`; saved shortly after a move or resize, on close and at exit; a position that is no longer on any screen is dropped. `examples/window-remember.html`.

#### Fixed
- Windows: a frameless window (`window-frame="solid"`) could show a system title bar for a moment, in the taskbar hover preview and when maximizing or restoring. Winit keeps `WS_CAPTION` on such windows and only hides the frame, so the system drew it whenever it rendered the frame itself; the style is now removed and kept off, and the system is told not to render a frame for the window at all (`DWMWA_NCRENDERING_POLICY`, except for `solid-with-shadow`), so the bar doesn't flash while the window opens or closes either (`snap_win`). Written without a Windows machine to test the fix on; type-checked only.
- Native message boxes (and the system clipboard) did nothing when a page used them while loading, e.g. `Window.this.modal(<alert>Hello</alert>)` in a `DOMContentLoaded` handler: the shell attached them after the page's first scripts had run, so `modal` quietly answered "yes". Pages now start with them (`Page::set_defaults`, `nile_shell::use_native`).

### 2026-10-19

#### Removed
- Browser navigation APIs: `history` (`pushState`, `replaceState`, `back`...), the `hashchange` event and writing to `location` (`href`, `hash`, `assign`, `replace`, `reload`). `location` stays as a read-only description of the page's URL. Nile is a desktop UI engine, not a browser; apps route with their own code, `new Window` or frames.

#### Added
- Linux menu bar: where the host has no system menu bar, `Window.this.menu = el` draws the same `<menu>` markup as a bar at the top of the page with dropdowns and submenus (mouse, arrows, Enter, Escape; items run like native ones, `role`s included). `NILE_MENU=inline` forces it elsewhere, `Page::set_native_menu(bool)` lets embedders choose. No Alt mnemonics yet.
- System tray icon and badge: `Window.this.tray = { icon, tooltip, menu, template }` (macOS `NSStatusItem`, Windows `Shell_NotifyIcon`; `trayclick`/`traydoubleclick` events; the menu is a `<menu>` like the app menu) and `application.badge` (macOS dock label, Windows taskbar overlay icon). Not on Linux yet. The Windows code was written without a Windows machine: check it in CI. `examples/tray.html`, `docs/guide/tray-and-badge.md`.
- `Clipboard` (Sciter's API: `read`, `write`, `readText`, `writeText`, `has`; also exported by `@nile`/`@sciter`): text, HTML, pictures (`ImageData`), copied files and in-app JSON, on the OS clipboard in the `nile` binary. The page, its frames and its scripts share one clipboard (`SharedClipboard`; `Page::set_clipboard`). The `Clipboard` trait gained `image`/`set_image`/`files`/`set_files`. `arboard` now builds with `image-data`. `examples/clipboard.html`.
- `media="platform != 'OSX'"` now works on any element, not only `<include>`: a false expression drops the element and its subtree when the page is parsed (set at run time by a script, it has no effect).
- Platform-specific window attributes: `window-frame-osx="native"` (also `-windows`, `-linux`, on any `window-*` attribute) overrides the plain attribute on that platform. `<include>` can't do this since it can't set attributes on `<html>`.

### 2026-10-18

#### Fixed
- Horizontal `margin` on inline elements (`<span style="margin-right: 20px">`) was ignored; it now takes space. Large inline padding or margin no longer makes the line taller (the spacer run is a one-pixel space stretched with letter spacing instead of a huge font).

#### Added
- HTML5 drag and drop (`examples/drag-drop.html`, [guide](docs/guide/drag-and-drop.md)): `draggable="true"` elements start a drag after a few pixels, with `dragstart`/`drag`/`dragenter`/`dragover`/`dragleave`/`drop`/`dragend`, `DataTransfer` (`setData`/`getData`, `effectAllowed`, `dropEffect`, `types`, `items`, `files`), a translucent copy of the element following the pointer, move/not-allowed cursors and Escape to cancel; files dropped from the desktop now fire `dragenter`/`dragover`/`drop` with `dataTransfer.files`. `nile --png` has `--drag X1,Y1,X2,Y2` / `--drop` to render a drag.
- Signed content updates (`examples/updater`, [guide](docs/guide/updates.md)): `nile --keygen` / `nile --release` make an Ed25519 key and a signed `update.json` plus the app's archive; a packed app calls `application.checkForUpdates()`, `downloadUpdate(update)` and `restartForUpdate()` (`application.version`, `updateReady`). The app's `nile.json` names the public key; the staged archive is verified (SHA-256, signature, newer version) at every start before it replaces the app's own files, so the runtime and the signed executable are never touched. `[hidden] { display: none }` is now in the user-agent sheet and `el.hidden` works (it did nothing before).
- App lifecycle (`examples/app-lifecycle.html`, [guide](docs/guide/app-lifecycle.md)): single instance (`<html app-single-instance>` or `"singleInstance"` in `nile.json`: a second launch hands its arguments to the first, which comes to the front and gets a `second-instance` event; checked before any script runs), `application.arguments` (in a packed app every plain argument is the program's, so a double-clicked file no longer replaces the start page), `open-url` and `open-file` events (macOS Apple events, also when they launch the app; `nile --pack --protocol scheme --file-type ext` or `"protocols"`/`"fileTypes"` in `nile.json` declare them in the `Info.plist` and the Linux `.desktop` entry), `application.registerProtocol()` / `registerFileType()` (Windows registry, Linux `.desktop` + `xdg-mime`) and `application.loginItem` (Run key, LaunchAgent, autostart entry). The Windows paths are written without a Windows machine to hand: CI compiles them.
- Audio (`examples/audio.html`, [guide](docs/guide/audio.md), new crate `nile-audio` over rodio/symphonia): Sciter's `Audio` class (`Audio.load(url)`, `play()` settling when the sound ends, `pause`/`resume`/`stop`, `volume`, `progress`) and the web `<audio>` / `new Audio(url)` (`play`, `pause`, `currentTime`, `duration`, `volume`, `muted`, `loop`, `playbackRate`, `autoplay`, the media events). MP3, Ogg Vorbis, FLAC, WAV and AAC/MP4 from files, a packed app or `data:` URLs. With no output device or `NILE_AUDIO=off` it plays silently against a clock. Linux builds now need `libasound2-dev`.
- Modern colour: `color-mix(in <space>, a [%], b [%])` (`srgb`, `srgb-linear`, `oklab`, `oklch`, `lab`, `lch`, `hsl`, `hwb`; premultiplied, shorter hue arc), `oklab()`, `oklch()`, `lab()`, `lch()`, `hwb()`, `color(srgb | srgb-linear | display-p3 | xyz ...)` and `light-dark(a, b)` (follows the element's own `color-scheme`).
- Math and units (`examples/css-modern.html`): `abs()`, `sign()`, `sqrt()`, `pow()`, `hypot()`, `log()`, `exp()`, `sin()`/`cos()`/`tan()`/`asin()`/`acos()`/`atan()`/`atan2()`, `round()` (with `up`/`down`/`to-zero`), `mod()`, `rem()`, `pi`, `e` inside `calc()` and friends (for numbers and `px`), `dvh`/`svh`/`lvh`/`dvw`/`svw`/`lvw` (the window size), `ch`/`ex`/`cap`/`ic`/`lh` (approximated from the font size) and `env(safe-area-inset-*)` (0 on a desktop window).
- CSS nesting (`&`, nested selectors with or without `&`, leading combinators, selector lists as the parent, nested `@media`/`@supports`) and `@layer` (named, nested, `@layer a, b;` ordering; unlayered rules beat layers, later layers beat earlier ones).
- `scroll-snap-type`, `scroll-snap-align` and `scroll-padding` (`examples/css-scroll-snap.html`): a container settles on a snap point once the wheel has paused for about 140 ms (`mandatory`, or `proximity` within 30% of the view); a gesture goes to the next point in its direction.
- Logical properties (`examples/css-logical.html`): `margin-/padding-/inset-/border-` `inline`/`block` (and `-start`/`-end`, `-width`/`-style`/`-color`), `inline-size`, `block-size` and their `min-`/`max-` forms, mapped to the physical sides for left-to-right horizontal text.
- CSS counters (`examples/css-counters.html`): `counter-reset`, `counter-increment`, `counter-set` and `counter()`/`counters()` in `content` (`::before`, `::after`, `::marker`), with `decimal`, `decimal-leading-zero`, `lower/upper-alpha`/`-latin`, `lower/upper-roman`; nested scopes follow the CSS rules.
- CSS visual effects (`examples/css-effects.html`): `filter` (`blur`, `brightness`, `contrast`, `grayscale`, `sepia`, `saturate`, `hue-rotate`, `invert`, `opacity`, `drop-shadow`, in any order), `backdrop-filter` (clipped to the rounded border box), `mix-blend-mode` (all 16 modes), `isolation: isolate`, `clip-path` (`circle()`, `ellipse()`, `inset()` without `round`, `polygon()`, reference box), `mask-image`/`-webkit-mask-image` with a gradient, and `object-fit`/`object-position` on images, canvases and SVG. A filter, blend mode, mask, clip-path or isolation makes the element a stacking context that paints as a unit, like `opacity`.
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
