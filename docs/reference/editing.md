# Rich-text and selection reference

[Reference index](README.md) · [Editing tutorial](../guide/rich-text-editing.md)

## Editing host and command results

Use `contenteditable="true"` on a document element. The engine maintains a DOM
caret/selection, modifies nodes, handles IME and provides editing history.
`plaintext-only` currently behaves as true; use textarea for enforced plain text.

`document.execCommand(name, false, value)` returns a boolean. A valid selection
inside an editing host is required for most commands. Focus and restore/create
the selection **before** invoking the command; check the return value before
reporting success. Command names are case-insensitive.

```js
const editor = document.getElementById('editor');
const range = document.createRange();
range.selectNodeContents(editor);
range.collapse(false);
editor.focus();
const selection = getSelection();
selection.removeAllRanges();
selection.addRange(range);
if (!document.execCommand('insertTable', false, '3x3')) {
  console.log('Table insertion failed');
}
```

Toolbar mousedown prevention should be delegated so dynamically created buttons
also preserve focus. Keep a cloned editor range when focus moves to a control;
validate both endpoint nodes remain inside the editor before restoring it.
The [Word script](../../examples/word-ribbon.js) implements this pattern.

## Command arguments

| Commands | Value/behavior |
|---|---|
| bold, italic, underline, strikeThrough, subscript, superscript | Toggle inline formatting; no value |
| foreColor | CSS color string |
| hiliteColor, backColor | Highlight/background color string |
| fontName | Font family string |
| fontSize | 1–7 map to 10, 13, 16, 18, 24, 32, 48 px; nonnumeric string passes through |
| fontSizePx | Exact pixel string/number (Nile extension) |
| removeFormat | Remove supported inline formats |
| formatBlock | Block tag such as p, h1–h6, blockquote |
| justifyLeft/Center/Right/Full | Paragraph alignment |
| insertUnorderedList, insertOrderedList | Toggle list structure |
| indent, outdent | Adjust margins, not a full nested-list model |
| blockStyle | CSS declaration string for selected blocks |
| insertText | Plain string |
| insertHTML | Markup string; app must decide whether content is trusted |
| insertParagraph, insertLineBreak | Paragraph split / line break |
| insertPageBreak | Break element for paged rendering |
| insertHorizontalRule | Horizontal separator, distinct from page break |
| insertImage | Supported local/packed/data image URL |
| createLink | URL string; unlink removes link wrappers |
| delete, forwardDelete | Delete backward/forward using supported selection behavior |
| selectAll | Select editing-host contents |
| undo, redo | Restore editing snapshots |
| copy, cut, paste | Queue clipboard request through host |
| insertTable | rows×columns, e.g. `3x2`; bounded to 50 rows and 20 columns per insertion |
| tableInsertRowAbove/Below | Add row around caret cell |
| tableInsertColumnLeft/Right | Add column around caret cell |
| tableDeleteRow/Column/Table | Remove corresponding structure |
| tableMergeCells | Merge selected rectangular cell region, maintaining spans |
| tableSplitCell | Split merged cell |

`queryCommandSupported`, `queryCommandEnabled`, `queryCommandState`,
`queryCommandValue` provide implementation-specific toolbar state. A supported
command can still fail without a suitable caret/selection/table context.

## Keyboard, tables and images

Typing, Backspace/Delete, arrows, Home/End, Shift selection extension,
Ctrl/Cmd+A/B/I/U/C/X/V/Z and redo shortcuts are implemented. Enter splits
paragraph/list items; Shift+Enter inserts a line break. Word/document movement
uses supported platform modifier conventions. Empty list items can exit lists.

Tab/Shift+Tab navigate table cells; Tab at the last cell appends a row. Enter
stays in a cell. Deletion across cells empties contents without merging their
structure. Row/column commands account for colspan/rowspan. Interactive column
resize in the editor is not supplied; a JS grid can implement its own widths.

Clicking an image selects it with eight handles. Corner drag preserves ratio
unless Shift is held; side handles resize a side. Size is stored in width/height
attributes and grouped into an undo step. Delete removes the selection;
arrows deselect it. Text wrapping/alignment around an edited image is limited.
See [image editing](../../examples/image-editing.html).

## Selection

`window.getSelection()` and `document.getSelection()` share the document
selection. Only one DOM range is supplied.

| Member | Contract |
|---|---|
| anchorNode/Offset, focusNode/Offset | Directional endpoints |
| isCollapsed, rangeCount, type, direction | Current selection state |
| toString() | Selected text |
| getRangeAt(0) | New Range representing supported DOM selection; can throw if no DOM range |
| addRange(range) | Replace the single current range |
| removeAllRanges, empty, removeRange | Clear selection |
| collapse(node,offset), setPosition | Set caret; null clears |
| collapseToStart/End | Collapse to ordered edge |
| extend(node,offset) | Move focus endpoint |
| setBaseAndExtent(anchor,offset,focus,offset) | Set directional selection |
| selectAllChildren(node), containsNode(node,partial) | Selection helpers |
| deleteFromDocument() | Execute delete command |

Opt-in page-text selection (`selectable`, user-select or htmlarea behavior) is a
separate native selection path. It can provide text/rangeCount without a full
DOM range retrievable by getRangeAt. Input/textarea selection uses its own
selectionStart/End API. Do not mix these models.

## Range

| Member group | Available API |
|---|---|
| Endpoints | startContainer/Offset, endContainer/Offset, collapsed, commonAncestorContainer |
| Set boundaries | setStart/End, setStart/EndBefore, setStart/EndAfter |
| Select/collapse | selectNode, selectNodeContents, collapse(toStart) |
| Copy/extract | cloneRange, cloneContents, extractContents, toString |
| Edit | deleteContents, insertNode, createContextualFragment |
| Compare | compareBoundaryPoints, comparePoint, isPointInRange, intersectsNode |
| Other | surroundContents (partial), detach (no-op), getBoundingClientRect (placeholder root rect) |

There is no accurate Range geometry/getClientRects implementation. Ranges are
node/offset snapshots, not fully live browser ranges automatically adjusted for
all mutations. Fragments use detached containers marked nodeType 11; insertion
moves their children. Cross-block surroundContents is incomplete.

## Events, history and clipboard

Cancelable `beforeinput` precedes supported edits; `input` follows successful
edits. `selectionchange` updates toolbar state. Input-event data/inputType are
supplied for supported operations. Use these for counts/state, not for rewriting
the entire editing host during every keystroke.

History snapshots cover the whole host. Replacing its DOM can invalidate cached
ranges and change history semantics. Typing/deletion grouping and image resize
steps are implemented, but this is not a full Word history model.

The shell writes clipboard HTML and plain text; paste keeps supported markup
and inline styles, dropping unsupported content. C clipboard callbacks exchange
text. Custom hosts provide clipboard services. Spell-check, text drag/drop,
advanced image wrapping and full browser editing compatibility are not supplied.
