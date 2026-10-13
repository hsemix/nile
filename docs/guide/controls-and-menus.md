# Controls, selection and custom menus

[Guide index](README.md) · Next: [Windows and embedding](embedding.md)

## Form controls

```html
<label for="name">Name</label>
<input id="name" placeholder="Your name">
<label><input id="updates" type="checkbox" checked> Activity updates</label>
<textarea id="notes">An editable note</textarea>
<select id="theme">
  <option value="light">Light</option>
  <option value="dark">Dark</option>
</select>
```

```js
document.getElementById('name').addEventListener('input', (event) => {
  console.log(event.target.value);
});
document.getElementById('theme').addEventListener('change', (event) => {
  document.body.classList.toggle('dark', event.target.value === 'dark');
});
```

Use `input` for updates while typing and `change` for committed control changes.
Read/write `.value`, `.checked`, `.disabled` and `.selectedIndex` as appropriate.
Radio buttons group by `name`. Labels can wrap their control or reference it with
`for`. `tabindex` affects Tab/Shift+Tab focus order. Use `.focus()`/`.blur()` and
`document.activeElement` for explicit focus management.

Text/password inputs and textareas support caret movement, selection,
clipboard shortcuts, undo/redo and word-wise movement/deletion. Selects support
keyboard navigation, type-ahead and a scrollable popup. For an inline list box:

```html
<select id="colors" multiple size="4">
  <option value="sage" selected>Sage</option>
  <option value="blue">Blue</option>
  <option value="sand">Sand</option>
  <option value="rose">Rose</option>
</select>
```

Read its chosen options with `select.selectedOptions.map((o) => o.value)`.
See [forms.html](../../examples/forms.html) and [listbox.html](../../examples/listbox.html).

Validation and submission belong to your script or host: automatic `<form>`
submission, browser validation and drag-and-drop are not implemented. IME
composition works in text controls and editable documents through the shell or
[host IME integration](embedding.md#input-methods-and-file-choosers).
Input selection indices currently count characters rather than
JavaScript UTF-16 code units. Values live in attributes rather than a separate
default-value model.

## Additional controls

Nile also renders number/range/color inputs, typed date/time/month/datetime-local
fields, file inputs, progress/meter, fieldsets, and details/summary disclosure.
See [inputs.html](../../examples/inputs.html), [number.html](../../examples/number.html)
and [controls2.html](../../examples/controls2.html).

Number controls step with arrows and respect min/max; range controls drag and emit
input/change. Date/time fields offer digit editing and field stepping, without a
calendar/clock popup. Color inputs accept hex text, without a color-picker popup.
File inputs request a native chooser in the shell; scripts read `.files` entries
with name/size/type and `.text()`. Other hosts must implement the chooser request
API. These controls do not add automatic form validation/submission.

## Enable page-text selection

Page text is not selectable by default, matching the desktop-UI use case.
Opt in for reading areas:

```html
<article class="document">Text the reader can select and copy.</article>
```

```css
.document {
  user-select: text;
  text-selection-background-color: #b9e6d9;
}
```

The `selectable` attribute and `behavior: htmlarea` also enable selection.
Drag to select, double-click for a word, triple-click for a text run, Shift+click
to extend, and use Ctrl/Cmd+A or Ctrl/Cmd+C. Read the selected string with:

```js
console.log(window.getSelection().toString());
window.getSelection().removeAllRanges();
```

For editable input text, use `selectionStart`, `selectionEnd`,
`setSelectionRange(start, end)` and `.select()` instead.

For editing, use `contenteditable` and the selection/command APIs documented in
[rich-text editing](rich-text-editing.md). Read-only page selection follows paint
order, cannot begin inside a link/control, and does not support Shift+arrow
extension. A `user-select: none` inline span inside a selectable text run is not
fully respected. Selection inside inputs/textareas remains separate from the
document selection.

## Build an application-drawn menubar

The runnable [menubar example](../../examples/menubar.html) draws File, View and
Help menus inside the window:

```sh
cargo run -p nile-shell -- examples/menubar.html
```

Its [script](../../examples/menubar.js) implements clicking, hover switching while
open, arrow-key navigation, Enter/Space activation, Escape, outside-click closing,
and Ctrl/Cmd+N. Disabled commands are skipped during keyboard navigation. View
toggles the details text and can rename the window. File creates a local draft
counter; disk saving is explicitly disabled. Close requests that the host close
the native window.

The core layout is:

```html
<div class="menu">
  <button>File</button>
  <div class="popup"><button>New draft</button></div>
</div>
```

```css
.menubar { position: relative; z-index: 10 }
.menu { position: relative }
.popup { display: none; position: absolute; top: 36px; left: 0; width: 220px }
.menu.open .popup { display: block }
```

JavaScript toggles `.open` on a menu, moves focus among enabled commands, and
calls the same command function for clicks and shortcuts. Keep visible shortcut
labels synchronized with the actual handlers. The example sets `aria-expanded`
and menu roles as markup, but a full platform accessibility bridge is still pending.
Tab follows regular focus order; the example does not implement the full browser
ARIA menu pattern, nested submenus or automatic repositioning at viewport edges.

The menubar creates a positioned stacking context above main content. A popup
cannot escape an ancestor's clipping/stacking constraints merely by increasing
its z-index. Outer-page menus also cannot currently cover embedded frames;
see [composition limits](composition.md#current-limits).

Custom menus are regular DOM elements, so they can be styled, checked, disabled
or given submenus using the same approach. Prefer popups outside overflow-clipped
containers. Native macOS/global OS menus require integration in the embedding host.

## Dialogs and command state

Use a positioned overlay and a panel for an application-drawn dialog. Manage its
open state in JavaScript, focus a useful control when it opens, restore focus on
close, and handle Escape and keyboard focus containment for production dialogs.
The [workbench shared controls](../../examples/workbench/core.js) show a simple
dialog with validation; it is a sample overlay, not a native modal dialog.

Prefer native `<button disabled>` over a visual-only disabled class. Treat
`role="menu"` and similar roles as semantic markup; they do not create control
behavior. Window chrome roles are an exception, handled by the shell as described
in [the window guide](embedding.md#configure-the-development-shell-window).
