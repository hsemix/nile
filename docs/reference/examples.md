# Runnable example catalog

[Reference index](README.md)

Run any HTML entry from the repository root:

```sh
cargo run -p nile-shell -- examples/pos/index.html
```

For a headless check, append `--png /tmp/example.png --width 1280 --height 820`.
Inspect terminal errors as well as the picture. Native menus/dialogs and window
ownership require a GUI run; PNG success does not verify them.

## Application examples

| Entry | Features/workflows | Persistence and limitations |
|---|---|---|
| [System theme](../../examples/system-theme/index.html) | Optional OS appearance, control states and components | External CSS package; [loading guide](../../themes/system/README.md) |
| [POS](../../examples/pos/index.html) | Product search/categories, quantities, discount, customers, held carts, cash/change, simulated card/mobile, receipts and reports | In-memory sample data; no real payments, database, authentication or printing. [Instructions](../../examples/pos/README.md) |
| [File explorer](../../examples/file-explorer/index.html) | Real directory tree, filtering, sorting, text preview, history, context menu and split panes | Read-only application; synchronous filesystem access, text previews ≤64 KB. [Instructions](../../examples/file-explorer/README.md) |
| [Virtual grid](../../examples/virtual-grid/index.html) | Up to 1M×100 logical cells, two-axis virtualization, jumps, keyboard navigation and sparse edits | Generated data; in-memory edits, fixed tracks. [Implementation](../../examples/virtual-grid/README.md) |
| [Spreadsheet](../../examples/excel/index.html) | Two sheets, formulas, cell editing/formatting, navigation and undo | Local saving; small fixed grid and documented formula subset. [Instructions](../../examples/excel/README.md) |
| [Word ribbon](../../examples/word-ribbon.html) | Live rich text, format/alignment/lists, search, clipboard, tables/images and print-layout view | Save/share/print buttons remain demos; page breaks use engine pagination. [Editing guide](../guide/rich-text-editing.md) |
| [Menubar](../../examples/menubar.html) | Nested custom menus, keyboard commands, editor, selection preservation and context menu | Draft in localStorage; not a native OS menu. [Script](../../examples/menubar.js) |
| [Workbench](../../examples/workbench/index.html) | Nine interfaces: settings, dashboard, kanban, mail, IDE, inventory, calendar, media, slides | In-memory sample flows; no real email/media/code execution. [Instructions](../../examples/workbench/README.md) |
| [Todo](../../examples/todo/index.html) | Modules, JSX, state and reconciliation | Small component example |
| [First app](../../examples/getting-started/index.html) | Inputs, events, selection, theme and basic layout | Tutorial entry |
| [Bootstrap](../../examples/bootstrap/run.js) | Hidden startup script opens application window | Native shell run required |
| [C host](../../examples/c/render.c) | Embedding, script/host bridge, input and rendering | Build instructions in [embedding](../guide/embedding.md) |

## Engine feature pages

Each page is a focused runnable example rather than a promise of complete
browser compatibility.

| Page | Demonstrates |
|---|---|
| [anim-events.html](../../examples/anim-events.html) | Transition/animation lifecycle events |
| [animations.html](../../examples/animations.html) | Transitions, keyframes and timing |
| [audit.html](../../examples/audit.html) | DOM/layout feature audit |
| [components.html](../../examples/components.html) | Class components and lifecycle |
| [controls2.html](../../examples/controls2.html) | Additional engine-drawn controls |
| [css-compat.html](../../examples/css-compat.html) | CSS compatibility checks |
| [dialogs.html](../../examples/dialogs.html) | Native and inline message/file dialogs |
| [editor.html](../../examples/editor.html) | Contenteditable, formatting, selection and history |
| [fetch.html](../../examples/fetch.html) | File requests, networking and storage |
| [fixed.html](../../examples/fixed.html) | Fixed/sticky positioning and scroll hit testing |
| [float.html](../../examples/float.html) | Floats, clear and wrapped text |
| [font-face.html](../../examples/font-face.html) | Local TTF/WOFF2 custom fonts |
| [forms.html](../../examples/forms.html) | Text controls, checkbox/radio, input events and selection |
| [frames-transformed.html](../../examples/frames-transformed.html) | Clipped/transformed frames |
| [frames.html](../../examples/frames.html) | Nested documents and frame messages |
| [frameset.html](../../examples/frameset.html) | Resizable native pane splitters |
| [gradients.html](../../examples/gradients.html) | Linear/radial/repeating gradients |
| [grid.html](../../examples/grid.html) | Grid tracks, alignment and placement |
| [hello.html](../../examples/hello.html) | Basic HTML/CSS rendering |
| [image-editing.html](../../examples/image-editing.html) | Image resize and editable table commands |
| [images.html](../../examples/images.html) | Local raster images and backgrounds |
| [ime.html](../../examples/ime.html) | Input-method composition |
| [include.html](../../examples/include.html) | Markup includes and media fallback |
| [inline.html](../../examples/inline.html) | Inline text layout and decorations |
| [inputs.html](../../examples/inputs.html) | Range, number, color, date/time and files |
| [inset-shadow.html](../../examples/inset-shadow.html) | Outer/inset box shadows |
| [interact.html](../../examples/interact.html) | Hover, active, focus and pointer interaction |
| [listbox.html](../../examples/listbox.html) | Multiple select/list scrolling |
| [live-regions.html](../../examples/live-regions.html) | Accessibility live announcements |
| [markers.html](../../examples/markers.html) | List markers, numbering and generated marker content |
| [measure.html](../../examples/measure.html) | Geometry and computed styles |
| [menubar.html](../../examples/menubar.html) | Custom popup-based menubar |
| [native-menu.html](../../examples/native-menu.html) | OS menu bar markup and accelerators |
| [nile-modules.html](../../examples/nile-modules.html) | Built-in module helpers |
| [number.html](../../examples/number.html) | Number stepping and input limits |
| [optgroup.html](../../examples/optgroup.html) | Grouped select options and disabled headings |
| [pagination.html](../../examples/pagination.html) | Page breaks, tables and PDF |
| [popups.html](../../examples/popups.html) | Popup positioning, context menus and detached portals |
| [position.html](../../examples/position.html) | Relative/absolute positioning |
| [print-layout.html](../../examples/print-layout.html) | Paged on-screen editable document |
| [pseudo.html](../../examples/pseudo.html) | Generated before/after content |
| [radius.html](../../examples/radius.html) | Corner radii and percentages |
| [scripting.html](../../examples/scripting.html) | DOM events and timers |
| [scrolling.html](../../examples/scrolling.html) | Overflow, wheel input and JS scroll |
| [selectors.html](../../examples/selectors.html) | Selector matching and state flags |
| [shape-outside.html](../../examples/shape-outside.html) | Geometric float outlines |
| [stacking.html](../../examples/stacking.html) | Z-index, opacity and stacking contexts |
| [static-position.html](../../examples/static-position.html) | Absolute elements with automatic offsets |
| [stress.html](../../examples/stress.html) | Large DOM performance workload |
| [subsup.html](../../examples/subsup.html) | Inline vertical alignment |
| [table-fixed.html](../../examples/table-fixed.html) | Explicit table column widths |
| [tables.html](../../examples/tables.html) | Semantic tables, spans and spacing |
| [transforms.html](../../examples/transforms.html) | 2D transforms |
| [transforms3d.html](../../examples/transforms3d.html) | Projected 3D transforms |
| [window.html](../../examples/window.html) | Window attributes and frameless chrome |
| [windows.html](../../examples/windows.html) | Multiple windows, modal result and shared messages |
| [word-ribbon.html](../../examples/word-ribbon.html) | Word-style editing ribbon |

[External CSS example](../../examples/css-file/index.html) demonstrates stylesheet links and imports.
Nested frame/include/window files support their parent demos and are not separate application entry points.
