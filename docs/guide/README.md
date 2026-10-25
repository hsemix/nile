# Using Nile

Nile draws desktop interfaces from HTML, CSS and JavaScript with its own rendering
pipeline. You can run a page in the `nile` development shell, render it without a
window, or embed the engine in an application that owns its window and services.
The authoring syntax is familiar, but the supported DOM and CSS are a subset;
Nile is not a browser or a drop-in implementation of Sciter.

## Feature reference

The [complete feature reference](../reference/README.md) documents API arguments,
return values, compatibility limits and runnable examples. It includes CSS/DOM,
web APIs, built-in modules, windows, rich text and every exported C function.

## Start here

1. [Build and run your first app](getting-started.md).
2. [Lay out and style an interface](layout-and-style.md).
3. [Add JavaScript, modules and JSX](scripting.md).
4. [Use controls, selection and custom menus](controls-and-menus.md).
5. [Edit rich text and connect a ribbon](rich-text-editing.md).
6. [Load data and persist drafts](data-and-services.md).
7. [Compose pages with includes, frames and splitters](composition.md).
8. [Configure a window or embed Nile](embedding.md).
9. [Make pages accessible to screen readers](accessibility.md).
10. [Print to PDF and control page breaks](printing.md).
11. [Ship an app as one file](packaging.md).
12. [Render and simulate input with the CLI](cli.md).
13. [Troubleshoot common problems](troubleshooting.md).
14. [Draw with `<canvas>`](canvas.md).
15. [Use SVG](svg.md).

All shell commands in this guide assume your current directory is the repository
root. Examples are source files you can edit and reload. No npm build is needed
for these examples.

## Example map

| Learn about | Open this example |
|---|---|
| POS sales, held orders, checkout and reports | [POS](../../examples/pos/README.md) |
| Large datasets with two-axis virtualization | [Virtual grid](../../examples/virtual-grid/README.md) |
| Real filesystem browsing | [File explorer](../../examples/file-explorer/README.md) |
| Optional OS-specific controls and components | [System theme](../../themes/system/README.md), [demo](../../examples/system-theme/index.html) |
| First app: input, events, theme, selectable text | [Getting started](../../examples/getting-started/index.html) |
| An application-drawn menubar | [Menubar](../../examples/menubar.html), [its script](../../examples/menubar.js) |
| Nine desktop interfaces with local state | [Workbench](../../examples/workbench/README.md) |
| Rich-text editing and a Word-style ribbon | [Editor](../../examples/editor.html), [Ribbon](../../examples/word-ribbon.html), [editing guide](rich-text-editing.md) |
| Excel-style grid, formulas and local saving | [Spreadsheet guide](../../examples/excel/README.md), [example](../../examples/excel/index.html) |
| IME composition | [IME](../../examples/ime.html) |
| Networking and storage | [Fetch](../../examples/fetch.html), [data guide](data-and-services.md) |
| Tray icon and badge | [Tray](../../examples/tray.html) |
| Clipboard (text, html, image, files, json) | [Clipboard](../../examples/clipboard.html) |
| Built-in modules | [Nile modules](../../examples/nile-modules.html) |
| Includes, frames and resizable panes | [Include](../../examples/include.html), [Frames](../../examples/frames.html), [Frameset](../../examples/frameset.html) |
| Fixed/sticky positioning and stacking | [Fixed](../../examples/fixed.html), [Stacking](../../examples/stacking.html) |
| Extended input types and file chooser | [Inputs](../../examples/inputs.html) |
| DOM events and timers | [Scripting](../../examples/scripting.html) |
| Forms, selection and clipboard shortcuts | [Forms](../../examples/forms.html) |
| Multiple selection and list boxes | [List boxes](../../examples/listbox.html) |
| Modules, JSX and reconciliation | [Todo](../../examples/todo/index.html), [app.jsx](../../examples/todo/app.jsx) |
| Scrolling and layout measurements | [Scrolling](../../examples/scrolling.html), [Measurements](../../examples/measure.html) |
| Flex, grid and table layout | [Hello](../../examples/hello.html), [Grid](../../examples/grid.html), [Tables](../../examples/tables.html) |
| Local images, transforms, generated content | [Images](../../examples/images.html), [Transforms](../../examples/transforms.html), [Pseudo-elements](../../examples/pseudo.html) |
| Gradients and animation | [Gradients](../../examples/gradients.html), [Animations](../../examples/animations.html) |
| Frameless windows | [Window](../../examples/window.html) |
| Drawer, splitter, data grid, barcodes, dialogs, toasts, shortcuts, tabs, accordion, combobox, ribbon, tiles, charts (`@nile/ui`) | [Drawer](../../examples/drawer.html), [Splitter](../../examples/splitter.html), [Data grid](../../examples/data-grid.html), [Barcodes](../../examples/barcodes.html), [Dialogs](../../examples/ui-dialogs.html), [Navigation](../../examples/ui-navigation.html), [Combobox](../../examples/ui-combobox.html), [Ribbon](../../examples/ui-ribbon.html), [Charts](../../examples/ui-charts.html) |
| C embedding | [render.c](../../examples/c/render.c), [generated header](../../crates/nile-capi/include/nile.h) |

For the broader feature list and remaining compatibility gaps, see the
[project README](../../README.md#whats-supported-today). For Rust API details,
generate the local crate documentation with `cargo doc --workspace --no-deps`;
the entry page is `target/doc/nile_engine/index.html`.
15. [Play sound with `Audio` and `<audio>`](audio.md).
16. [Copy and paste text, images and files](clipboard.md).
17. [Tray icon and dock/taskbar badge](tray-and-badge.md).
16. [Run one instance, open links and files, start at login](app-lifecycle.md).
17. [Publish signed updates for a packed app](updates.md).
18. [Drag and drop elements and files](drag-and-drop.md).
19. [Ready-made components: drawer, splitter, data grid, dialogs, toasts, shortcuts, tabs, accordion, combobox, ribbon, tiles, charts, barcodes and QR codes (`@nile/ui`)](ui-components.md).
20. [Printing: the page as paper, PDF, printers and ESC/POS receipt printers (`@nile/print`, `@nile/escpos`)](printing.md).
21. [Hashing, password hashing and encryption: `crypto.subtle` and `@nile/crypto`](hashing-and-encryption.md).
