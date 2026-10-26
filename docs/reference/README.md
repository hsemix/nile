# Nile feature reference

This reference describes the implementation in this repository, audited on
2026-10-14. Nile implements a desktop HTML/CSS/JS runtime; familiar browser or
Sciter names do not imply full compatibility. Each page distinguishes working
behavior, partial implementations and host responsibilities.

Start with the [tutorials](../guide/README.md) to build an application. Use these
pages to look up features, arguments, return values and limitations.

| Reference | Contents |
|---|---|
| [CSS](css.md) | Selectors, cascade, units, layout, painting, text, animations and supported property names |
| [DOM and events](dom.md) | Documents, nodes, attributes, queries, measurements, event phases, controls, JSX and timers |
| [Web APIs](web-api.md) | Fetch/XHR, binary data, streams, WebSocket, storage, observers, URLs and navigation |
| [Built-in modules](modules.md) | `@nile`, `@sciter`, `@env`, `@sys`, `@storage`, `@debug`, `@nile/ui` and application bootstrap |
| [Windows and menus](windows.md) | Window configuration, multiple windows, messages, dialogs, native menus, popups and tooltips |
| [Editing](editing.md) | Rich-text command arguments, selection, ranges, tables, images and clipboard |
| [Native integration](native-api.md) | Rust host lifecycle and every exported C function, with ownership and safety contracts |
| [Large interfaces](performance.md) | Scrolling, virtualization, scheduling and layout measurements |
| [Example catalog](examples.md) | Runnable demonstrations and what each implements |
| [What's supported](supported.md) | The complete inventory of supported features, one dense list |
| [Known gaps](known-gaps.md) | What is missing or partial, and why |

## Feature coverage

| Feature | Where to learn and look it up |
|---|---|
| Build, run, reload, snapshots and simulated input | [Getting started](../guide/getting-started.md), [CLI](../guide/cli.md) |
| HTML parsing, style sheets, imports, includes | [CSS](css.md), [composition](../guide/composition.md) |
| Box model, normal flow, inline layout | [CSS layout](css.md#layout) |
| Flex, grid, tables and column sizing | [CSS layout](css.md#layout) |
| Float, clear and shape outlines | [CSS layout](css.md#layout) |
| Relative, absolute, fixed, sticky, stacking | [CSS positioning](css.md#positioning-and-scrolling) |
| Overflow, scrollbars, programmatic scroll | [DOM measurements](dom.md#measurements-and-scrolling), [large interfaces](performance.md) |
| Fonts, `@font-face`, fallback, text decoration and lists | [CSS text](css.md#text-and-fonts) |
| Images, gradients, shadows, radius and opacity | [CSS painting](css.md#painting) |
| 2D/3D transforms, transitions and animations | [CSS motion](css.md#transforms-and-motion) |
| Generated content, pseudo-elements, custom properties | [CSS selectors/cascade](css.md#selectors-and-cascade) |
| Script loading, modules, JSX, function/class components | [DOM components](dom.md#jsx-and-components), [scripting tutorial](../guide/scripting.md) |
| Node creation, traversal, queries and mutation | [DOM](dom.md) |
| Capture, target, bubble, once and event cancellation | [Events](dom.md#events) |
| Input, buttons, textarea, checkbox, radio, select and optgroup | [Control reference](dom.md#controls-and-form-values), [controls guide](../guide/controls-and-menus.md) |
| Number/range/color/date/file controls and details/progress/meter | [Control reference](dom.md#controls-and-form-values) |
| Focus, Tab order, keyboard shortcuts, selection and IME | [Controls](dom.md#controls-and-form-values), [editing](editing.md) |
| Rich text, tables, image resize, clipboard, undo/redo | [Editing](editing.md) |
| Files, directories, processes, TCP, TLS, UDP and OS queries | [Modules](modules.md) |
| Network requests, WebSocket, FormData and streams | [Web APIs](web-api.md) |
| Local/session storage and JSON object storage | [Web APIs](web-api.md#storage), [modules](modules.md#storage) |
| Mutation, resize, intersection and media observers | [Web APIs](web-api.md#observers-and-media) |
| Window settings, chrome, fullscreen and multiple windows | [Windows](windows.md) |
| Native/inline dialogs, popup portals, tooltips and context menus | [Windows](windows.md) |
| Includes, frames, messages and frameset splitters | [Composition](../guide/composition.md) |
| Page breaks, `@page`, margin boxes, paged editing and PDF | [Printing](../guide/printing.md), [CSS](css.md#pagination) |
| Accessibility roles, names, actions, text and live regions | [Accessibility](../guide/accessibility.md) |
| Rust/C embedding, host calls, files, IME and windows | [Native integration](native-api.md), [embedding tutorial](../guide/embedding.md) |
| Packed assets, executable branding, icons and bootstrap | [Packaging](../guide/packaging.md), [modules](modules.md#application-bootstrap) |
| Virtual lists and two-axis grids | [Large interfaces](performance.md) |
| POS, file explorer, spreadsheet, Word ribbon and workbench | [Example catalog](examples.md) |

## Compatibility and capabilities

Unsupported CSS is ignored; unsupported selector rules can be discarded as a
whole. Some exposed APIs are compatibility placeholders, listed on their
reference pages. Do not treat API existence as proof of browser-equivalent
behavior. In particular, this runtime is not a browser security sandbox:
`@sys`, `@env` and host services can access the machine.

The engine owns layout, rendering and input. The development shell supplies OS
windows, native menus/dialogs and platform integration. A custom host must
implement those services and drive the engine's tick/input/paint lifecycle.
SQLite, payment processing and printer drivers belong in the application host;
they are not built-in Nile APIs.

When adding a feature, update its reference, limitation and runnable example.
The [C reference](native-api.md) mirrors exported source documentation; the
[CSS name inventory](css.md#supported-property-name-inventory) mirrors the
cascade's property dispatch, including grid and animation declarations.
