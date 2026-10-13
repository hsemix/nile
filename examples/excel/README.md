# Nile spreadsheet example

An Excel-style interface built with Nile's existing HTML/CSS/JS APIs. It has two
worksheets, an editable grid, a formula bar and a small JavaScript calculation
engine. No engine changes or npm dependencies are required.

```sh
cargo run -p nile-shell -- examples/excel/index.html
```

## Edit and navigate

- Click a cell to select it. Double-click, press F2/Enter, or start typing to edit.
- Enter commits an edit and moves down; Shift+Enter moves up. Tab/Shift+Tab move
  right/left. Escape discards an uncommitted edit.
- The formula bar shows the raw value/formula, while the grid shows its result.
  Edit the bar and press Enter or its apply button. Changing cells commits the
  pending formula-bar edit; its cancel button discards it.
- Arrow keys navigate when the grid is focused. Inside an editor they move the
  caret normally. Delete/Backspace clear the selected cell when not editing.
- Type an address such as `B5` into the name box and press Enter to navigate.
- Budget and Forecast have independent values, formatting and undo histories.
- Ribbon formatting changes the selected cell: bold, fill, alignment, currency
  and percentage display. Clear removes a cell value; Clear formatting preserves it.

## Formulas

Formulas start with `=`. References are case-insensitive within `A1:H24` on the
current sheet. Editing a referenced cell recalculates its dependents.

| Example | Result/behavior |
|---|---|
| `=B5+C5+D5` | Add three cells |
| `=(B5+C5)*2` | Arithmetic with parentheses |
| `=SUM(B5:D5)` | Sum numeric cells in a range |
| `=AVERAGE(B5:D5)` | Average numeric cells |
| `=MIN(B5:D5)` / `=MAX(B5:D5)` | Minimum/maximum |
| `=COUNT(B5:D5)` | Count numeric cells |
| `=SUM(B5:D5,100)` | Combine ranges and numeric expressions |

Arithmetic supports `+`, `-`, `*`, `/`, unary signs and decimals. Aggregate ranges
ignore text and blanks; arithmetic treats blank references as zero and text
references as `#VALUE!`. Errors include `#DIV/0!`, `#REF!`, `#CYCLE!` and `#ERROR!`.
Errors propagate to dependents. Formulas are parsed directly, never executed as JS.
AutoSum/Average use the contiguous numeric cells above the selected cell.

## Saving and undo

Click the save icon, File → Save workbook, or Ctrl/Cmd+S to persist both worksheets
in localStorage. Reloading the same page restores the saved workbook. Saving is
explicit; unsaved changes are lost on reload. `NILE_STORAGE_DIR` controls disk
storage, and `NILE_STORAGE_DIR=off` disables automatic persistence for test runs.

Ribbon undo/redo restores committed values and formatting, with up to 100 changes
per worksheet. Grid Ctrl/Cmd+Z and Shift+Ctrl/Cmd+Z (or Ctrl/Cmd+Y) use workbook
history. While editing a cell/formula, native text undo applies to that input;
commit before using workbook undo. Reset this sheet restores its sample and can
itself be undone.

## Scope

This is a small spreadsheet demo, not Excel file compatibility. It has a fixed
8-column, 24-row grid; no range selection, drag fill, cross-sheet references,
relative-reference copying, sorting/filtering, charts, merged cells, XLSX/CSV
import/export or frozen headers. Clipboard shortcuts edit text inside the active
input rather than transferring a cell range. Currency display rounds to whole
dollars, general format displays up to four decimal places, and percentages to
one decimal place. Calculations use JavaScript floating-point numbers.

## Render a preview

```sh
NILE_STORAGE_DIR=off cargo run -p nile-shell -- examples/excel/index.html --png /tmp/nile-excel.png --width 1280 --height 900 --scale 2
```

Inspect the image and terminal output; Nile can still write a PNG after a
JavaScript error.

Source: [interface](index.html), [application](app.js), [formula parser](formulas.js).
Reference preview: [spreadsheet.png](../../docs/spreadsheet.png).
