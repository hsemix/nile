# Rich-text editing and the Word ribbon

[Guide index](README.md) · Related: [Controls](controls-and-menus.md)

## Create an editor

```html
<button id="bold">Bold</button>
<article id="editor" contenteditable="true">
  <h2>Meeting notes</h2>
  <p>Select some text, then click Bold.</p>
</article>
<script>
  const bold = document.getElementById('bold');
  // Keep the editor's focus and selection when clicking the toolbar.
  bold.addEventListener('mousedown', (event) => event.preventDefault());
  bold.addEventListener('click', () => document.execCommand('bold'));
  document.addEventListener('selectionchange', () => {
    bold.classList.toggle('selected', document.queryCommandState('bold'));
  });
</script>
```

This edits the actual document DOM. Formatting applies to the selected text;
with a collapsed caret, inline formatting applies to subsequently typed text.
Use `<textarea>` when you need plain text only. `contenteditable="plaintext-only"`
currently behaves as `true`, so it does not enforce plain-text editing.

Run the complete examples:

```sh
cargo run -p nile-shell -- examples/editor.html
cargo run -p nile-shell -- examples/word-ribbon.html
```

## Commands and toolbar state

Call `document.execCommand(name, false, value)`. Check its return value, and use
`queryCommandSupported`, `queryCommandEnabled`, `queryCommandState` and
`queryCommandValue` to determine support and toolbar state.

| Purpose | Commands / example value |
|---|---|
| Inline emphasis | `bold`, `italic`, `underline`, `strikeThrough`, `subscript`, `superscript` |
| Text color / highlight | `foreColor`, `hiliteColor` with `'#fff0aa'` |
| Font | `fontName` with `'serif'`; `fontSize` with `'1'`–`'7'` |
| Exact pixel size (Nile extension) | `fontSizePx` with `'18px'` |
| Paragraphs | `formatBlock` with `'h2'`, `'p'` or `'blockquote'`; `justifyLeft`, `justifyCenter`, `justifyRight`, `justifyFull` |
| Lists | `insertUnorderedList`, `insertOrderedList`, `indent`, `outdent` |
| Selected-block styles (Nile extension) | `blockStyle` with `'line-height: 1.8; margin-left: 16px'` |
| Insertion | `insertText`, `insertHTML`, `insertParagraph`, `insertLineBreak`, `insertImage`, `insertHorizontalRule`, `insertPageBreak` |
| Links / cleanup | `createLink`, `unlink`, `removeFormat` |
| Editing | `delete`, `selectAll`, `undo`, `redo`, `copy`, `cut`, `paste` |
| Tables (Nile extension) | `insertTable` with `'3x2'`; with the caret in a cell: `tableInsertRowAbove`, `tableInsertRowBelow`, `tableInsertColumnLeft`, `tableInsertColumnRight`, `tableDeleteRow`, `tableDeleteColumn`, `tableDeleteTable`, `tableSplitCell`, and `tableMergeCells` (select from one cell to another; the covered rectangle becomes one cell with `colspan`/`rowspan`) |

Use the supported command API for edits instead of rewriting the editor's whole
`innerHTML` during typing. Undo snapshots cover the entire editing host, so script
rewrites also affect its history. Selection indices and Range boundaries refer
to specific DOM nodes; keep those nodes alive while using the selection.

Enter splits paragraphs/list items, Shift+Enter inserts a line break, and an
empty list item can exit the list. Arrow keys, Home/End and modifier shortcuts
move/extend the caret selection. Ctrl/Cmd+B/I/U toggle formatting; Ctrl/Cmd+A,
C/X/V and undo/redo shortcuts are supported. IME composition works in the editor.
Table Tab/Shift+Tab move between cells; Tab from the last cell adds a row.
Enter stays inside a cell and deleting across cells empties their content. Row and column commands
understand merged cells (spans grow or shrink where a line crosses them).

Clicking an image selects it (outline and eight handles). Dragging a corner resizes it in proportion
(Shift frees the ratio), an edge handle changes one side; the size is stored in the `width`/`height`
attributes and is one undo step. Backspace/Delete removes the selected image, and the arrow keys
deselect it.

## Read and change the selection

```js
const editor = document.getElementById('editor');
editor.focus();
const range = document.createRange();
range.selectNodeContents(editor);
const selection = window.getSelection();
selection.removeAllRanges();
selection.addRange(range);
console.log(selection.toString());
```

Selection exposes anchor/focus nodes and offsets, `rangeCount`, `getRangeAt`,
`collapse`, `extend`, `setBaseAndExtent` and `selectAllChildren`. Range supports
setting boundaries, selecting contents, text extraction, deletion and insertion,
plus cloning/extracting contents and boundary comparisons. It is a partial API;
`Range.getClientRects` and cross-block `surroundContents` are not implemented.
Input/textarea selection is separate; use their `selectionStart`/`selectionEnd`.

Editors emit cancelable `beforeinput`, followed by `input` for edits, and
`selectionchange` for selection changes. Update counts on `input` and toolbar
state on `selectionchange`. Handle both without replacing the editing host.

## What the ribbon example does

The existing [ribbon script](../../examples/word-ribbon.js) uses `execCommand` on
the live document selection. It supports inline formatting, paragraph styles,
alignment, lists, indentation, line spacing, borders, clipboard commands,
undo/redo, selecting all, word count and search. Insert → Table inserts a 3×3
table. Search matches within text nodes and advances on Enter; a match spanning
multiple styled nodes is not found as one string.

Several ribbon items remain demonstrations: Save and AutoSave do not persist the
document, sharing is not implemented, and Print preview is not connected to PDF export. Insert → Page break uses
`insertPageBreak`; View → Print layout enables paged rendering. Paragraph marks toggles
an outline, not visible nonprinting characters. For persistence, explicitly save
the editor's `innerHTML` with [storage](data-and-services.md#save-a-draft).

## Current limits

Copy writes HTML and plain text in the shell. Paste retains a supported subset of
HTML (emphasis, headings, lists, quotes, links, images and selected inline styles).
C clipboard callbacks currently exchange text only. There is no spell-check,
text drag-and-drop, image alignment/wrapping or captions, or resizing table columns by dragging. Indent uses margins rather than nested
lists. This is an editable document foundation, not full Word compatibility.

See the [editing reference](../reference/editing.md) for every command argument,
Selection/Range member and geometry limitation.
