# Pages, printing and PDF

Nile can cut a document into sheets of paper and write them to a PDF. The page is still laid out as
one long column; pagination only chooses where each sheet starts, so the same HTML shows on screen
and on paper.

```sh
cargo run -p nile-shell -- examples/pagination.html --pdf report.pdf
cargo run -p nile-shell -- examples/pagination.html --png page2.png --page 2 --scale 2
```

`--pdf` writes every page (text and shapes stay vector). `--page N` with `--png` renders page `N`
(1-based) as a picture so you can check a layout without opening a PDF viewer.

## `@page`

```css
@page {
  size: A4;                       /* A3 A4 A5 B4 B5 letter legal ledger, or "8in 10in"; add landscape */
  margin: 0.8in 0.9in;            /* one to four lengths: px in cm mm pt pc q */
  @top-right    { content: "Annual Report"; font-size: 9pt; color: #888 }
  @bottom-center { content: "Page " counter(page) " of " counter(pages) }
}
@page :first { margin-top: 2in }   /* :first, :left, :right */
```

Margin boxes `@top-left|center|right` and `@bottom-left|center|right` take a `content` of strings and
`counter(page)` / `counter(pages)`, plus `font-size` and `color`. Without an `@page` rule the paper is
A4 with 0.75 inch margins.

## Where pages break

- `break-before`, `break-after`: `page` (also `always`, `left`, `right`) forces a break; `avoid` forbids one.
- `break-inside: avoid` keeps an element on one page when it fits on one. The `page-break-*` spellings work too.
- A page never cuts through a text line, a table row or a picture, and keeps at least two lines of a
  paragraph on each side of a cut (as `orphans: 2; widows: 2` would).
- Content taller than a page is cut at the page edge, between lines where it can.
- A table with a `<thead>` repeats its header row(s) at the top of every page it continues onto (in the PDF and in `page-view: paged`); a page never ends right after the header. A `<tfoot>` is shown again at the bottom of every page its table goes on from (the last page shows the real one).

## A paged view on screen

`page-view: paged` shows one element's content as the sheets it will print on, inside your own
scrolling layout (a desk with a ribbon above it, say):

```css
@page { size: 720px 930px; margin: 57px 70px; @bottom-center { content: "Page " counter(page) " of " counter(pages) } }
.desk  { overflow: auto; background: #edf0f5 }
.paper { page-view: paged; margin: 0 auto; background: #fff; box-shadow: 0 2px 10px #0003 }
```

The element becomes one sheet wide and as tall as its sheets (with a 20px gap), takes the `@page`
margins as its padding and draws its own background, border and shadow once per sheet. Everything
else works as on a flat page: the text is still one document, so a `contenteditable` paper can be
typed in, selected and clicked across sheet boundaries, and the arrow keys step over the margin and
the gap. Clicking in a margin puts the caret at the nearest line. `el.pageCount` and `el.pageNumber`
(the sheet with the caret, else the one in the middle of the window) give a status bar its
"Page 2 of 5", and `document.execCommand("insertPageBreak")` starts a new sheet in the editor.
`examples/print-layout.html` is the minimal page; the View tab of `examples/word-ribbon.html` turns it on.

## From a host

`Page::paginate()` returns the page count, `Page::render_page(index, scale)` gives BGRA pixels,
`Page::write_pdf(&mut writer, title)` writes a PDF, and C hosts call `nile_view_write_pdf(view, path)`.

## Not yet

- In the paged view: frames inside the paper, transforms on the paper itself, content taller than a sheet (it is cut at the sheet edge), and a paper that is also a scroll container.
- A repeated header for tables nested in tables, `position: fixed` headers per page; `@page` margin-box sizing beyond the six corners/edges.
- `@media print` rules, and printing through the system's print dialog (export to PDF and print that).
- Pages that have their own `size` or margins (`@page :first`) flow at the first page's content size.
