# Ready-made components: `@nile/ui`

`@nile/ui` is an optional library of components for desktop pages, written in JavaScript on top of
the engine (custom elements, SVG, modules). It is built into the program, so there is nothing to
install or ship: import what you use. A module that isn't imported costs nothing.

```js
import { svg, encode, dataUrl } from "@nile/ui";   // everything
import { svg } from "@nile/ui/barcode";            // one module
//   modules: barcode, drawer, splitter, grid
```

Importing a module registers its elements, so after `import "@nile/ui"` the tags below work in
the page.

## Drawer

A panel that slides in from an edge of the window: a menu, a settings sheet, a details pane.

```html
<button data-drawer-toggle="#admin">&#x22EE;</button>

<nile-drawer id="admin" size="340">
  <h2>POS - Admin <button data-drawer-close>&#x2192;</button></h2>
  ...
</nile-drawer>
```

Attributes: `side` (`right` by default, `left`, `top`, `bottom`), `size` (px or any CSS length, default 320),
`open` (present = shown), `backdrop` (`false` leaves the page behind usable and un-dimmed), `dismissible`
(`false` turns off Escape and the click-outside close). Methods: `show()`, `hide()`, `toggle()`; property
`opened`. Events: `open`, `close`, and a cancelable `beforeclose` (`preventDefault()` keeps it open, for
unsaved changes).

Buttons can drive it without script: `data-drawer-toggle="#id"`, `data-drawer-open="#id"`, and
`data-drawer-close` (no value = the drawer the button is in). While a modal drawer is open Tab stays inside
it, Escape closes it, and focus returns to where it was. Style it with `--nile-drawer-bg`,
`--nile-drawer-color`, `--nile-drawer-shadow`, or target `.nile-drawer-panel` directly. See
`examples/drawer.html`.

## Splitter

Resizable panes with a draggable grip (with the three-dot handle) between them. It takes the same
size units as `<frameset>` (a number or `px`, `%`, and `N*` shares of the leftover space), so it can do
everything the frameset's `<splitter>` does and more: use it for new pages, and keep `<frameset>` for
Sciter pages you are porting.

```html
<nile-splitter style="flex: 1" persist="main-layout">
  <div size="300" min="180" max="500" collapsible>sidebar</div>
  <div>content</div>
</nile-splitter>

<!-- or a list on the host, like a frameset (cols = side by side, rows = stacked) -->
<nile-splitter cols="200px 1* 30%">...three panes...</nile-splitter>
```

On the host: `cols` or `rows` (a list of sizes, one per pane; `rows` stacks them), `orientation`
(`horizontal` = side by side, the default; `vertical` = stacked), `grip` (`dots`, `line` or `none`), `persist`
(a name: sizes and collapsed panes are remembered in `localStorage`). On the panes: `size` (`200`, `200px`,
`30%`, or `2*` for two shares of the leftover space; a pane without one is `1*`; it overrides the host's
list), `min` (px, default 40), `max` (px), `collapsible`, `collapsed`. As with a frameset, dragging a grip
turns the panes beside it into pixel sizes.

Drag the grip, or focus it and use the arrow keys (Shift for bigger steps). A plain click, Enter or Space
collapses or expands the neighbouring `collapsible` pane, and expanding restores the size it had. Methods:
`setSize(i, px)`, `collapse(i)`, `expand(i)`, `toggle(i)`, `isCollapsed(i)`, `refresh()` (after adding or
removing panes); property `sizes`; event `splitterchange` (`detail: { sizes, collapsed }`). Style with
`--nile-splitter-grip` (thickness), `--nile-splitter-bg`, `--nile-splitter-hover`, `--nile-splitter-color`.
Splitters nest. See `examples/splitter.html`.

## Data grid

A list of records with columns, sorting, selection and keyboard navigation. Only the rows in view are in the
document, so 100,000 rows cost about what 30 do.

```html
<nile-grid id="receipt" key="id" empty="No items" empty-hint="Scan a product">
  <nile-column key="name" title="Product name"></nile-column>
  <nile-column key="qty" title="Quantity" width="90" type="number"></nile-column>
  <nile-column key="price" title="Price" width="90" type="number"></nile-column>
</nile-grid>
```
```js
receipt.rows = [{ id: 1, name: "Espresso", qty: 2, price: 3.5 }];
receipt.on("rowactivate", (e) => edit(e.detail.row));       // Enter or double-click
receipt.on("selectionchange", (e) => show(e.detail.selected));
```

Columns can also be given from script, which allows computed and formatted cells:

```js
grid.columns = [
  { key: "name", title: "Product" },
  { key: "price", title: "Price", width: 90, type: "number", format: (v) => v.toFixed(2) },
  { key: "amount", title: "Amount", type: "number", value: (r) => r.qty * r.price },
  { key: "x", title: "", width: 40, render: (row) => { const b = document.createElement("button"); b.textContent = "x"; return b; } },
];
```

| Column field | Meaning |
|---|---|
| `key`, `title` | The record field and the header text |
| `width` | Pixels. A column without one shares the leftover space |
| `minWidth` | Smallest width when resized or squeezed (default 60) |
| `type: "number"` | Right-aligned, sorts numerically |
| `align` | `left`, `right` or `center` |
| `value(row)` | Computed value instead of `row[key]` (used for sorting too) |
| `format(value, row)` | Text to show |
| `render(row, value, index)` | A string, a DOM node or `{ html }` for custom cells (buttons, badges) |
| `compare(a, b)` | Your own sort order |
| `sortable`, `resizable` | `false` to turn off |

Attributes on the grid: `row-height` (px, default 32), `select` (`none`, `single` or `multiple`; default
`single`), `key` (the field that identifies a record, so the selection survives replacing `rows`), `sort`
(`"name"` or `"name:desc"`), `striped`, `header="false"`, `empty` / `empty-hint`.

Properties: `rows`, `columns`, `selected` (the selected records, in grid order), `selectedIndex`,
`selectedCount` (cheap, unlike `selected` on a huge selection), `rowCount`, `mountedCount` (rows drawn right now),
`sortKey`, `sortDirection`. Methods: `add(row)`, `remove(rowOrIndex)`, `update(index, patch)`, `clear()`,
`select(rowOrIndex, ...)`, `sortBy(key, "asc"|"desc")`, `scrollToRow(i)`, `refresh()` (after changing
records in place). Events: `selectionchange`, `rowactivate`, `sort`, `columnresize`.

Rows that scroll out of view are reused for the ones scrolling in (so keep per-scroll work in your own handlers
light: a handler that loops over all `rows` on every `scroll` event will stutter any list), and each row
carries `aria-rowindex`. Click a header to sort (ascending, descending, then back to the data order); "Item 2" sorts before
"Item 10", and blank values always go last. Click selects; with `select="multiple"` Shift-click extends,
Ctrl/Cmd-click toggles, Space toggles and Ctrl/Cmd+A selects all. Arrow keys, Page Up/Down, Home and End
move, and the grid scrolls to keep the row visible. Drag a header's right edge to resize the column. Clicks
on buttons, links and inputs inside a cell don't change the selection.

Sorting happens in memory (100,000 rows take about half a second the first time). For data that lives
elsewhere, load it in pages into `rows` and handle the `sort` event yourself. Style with `--nile-grid-bg`,
`--nile-grid-color`, `--nile-grid-head-bg`, `--nile-grid-border`, `--nile-grid-hover`, `--nile-grid-stripe`,
`--nile-grid-selected-bg` and `--nile-grid-selected-color`. Not included: editing cells in place, variable
row heights, column reordering, grouping. See `examples/data-grid.html`.

## Barcodes

```html
<nile-barcode type="code128" value="ORDER-1042" height="50"></nile-barcode>
<nile-barcode type="ean13" value="590123412345"></nile-barcode>
<nile-barcode type="qr" value="https://example.com/orders/1042" module-width="4"></nile-barcode>
```

```js
import { svg, encode, dataUrl, types } from "@nile/ui";

el.innerHTML = svg("ean13", "5901234123457", { height: 44, moduleWidth: 2 });
img.src = dataUrl("code128", "SKU-77");     // for <img>, CSS backgrounds, printing
encode("qr", "hello", { ecc: "H" });         // { size, version, rows: ["1110...", ...] }
```

| `type` | What it is | Value |
|---|---|---|
| `code128` | Any text (ASCII 0-126), switches between code sets to keep digits compact | text |
| `code39` | Upper-case letters, digits, `- . space $ / + %` | text |
| `ean13` | Retail (ISBN/EAN) | 12 digits (check digit added) or 13 (checked) |
| `ean8` | Small retail | 7 or 8 digits |
| `upca` | North American retail | 11 or 12 digits |
| `qr` | QR code, byte mode (UTF-8), versions 1 to 40 | any text |

A value that can't be encoded (a wrong check digit, a letter in an EAN, text that is too long for
a QR code) throws an `Error` from `svg`/`encode`; the element shows the message instead and gets
an `invalid` attribute.

**Options** (attributes on the element, or properties of the options object):

| Option / attribute | Default | Meaning |
|---|---|---|
| `height` | `60` | Bar height in px (linear codes) |
| `moduleWidth` / `module-width` | `2` (QR: `4`) | Width of the narrowest bar in px. Use a whole number: bars on whole pixels are what scanners need |
| `margin` | `10` (QR: `4`) | Quiet zone, in modules, on each side |
| `color`, `background` | `#000`, `#fff` | `background="none"` for transparent |
| `text` | on | `text="false"` hides the digits under linear codes |
| `fontSize` / `font-size` | by size | Size of the digits |
| `ecc` | `M` | QR error correction: `L`, `M`, `Q` or `H` |
| `version` | smallest that fits | Smallest QR version to use (1 to 40) |

EAN-13, EAN-8 and UPC-A print their digits in the standard layout (first digit outside the bars,
one group under each half).

### Checked against other encoders

The encoders are plain JavaScript and were compared with independent implementations: QR output
is module-for-module identical to the `qrcode` Python library for every version (1-40) at every
error-correction level; EAN-13, EAN-8, UPC-A and Code 39 match `python-barcode` bar for bar;
Code 128 decodes back to its text with a valid check digit; and pages rendered by Nile were read
back by OpenCV's barcode and QR readers. Scan a printed label with your own scanner before
relying on it, since printers and sizes matter.

Not included: QR numeric/alphanumeric/Kanji modes (everything is byte mode, so codes can be a
little larger than another generator's), Micro QR, Data Matrix, PDF417, Code 128 FNC characters,
Code 39 check digits, EAN add-ons.

See `examples/barcodes.html`; the price tags in `examples/retail-counter` use the same EAN-13
encoder.
