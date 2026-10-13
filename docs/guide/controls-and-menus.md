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

The redesigned [menubar example](../../examples/menubar.html) provides File,
Edit, View and Help menus around an editable notes workspace:

```sh
cargo run -p nile-shell -- examples/menubar.html
```

Its [script](../../examples/menubar.js) uses `element.popup()` for anchored menus,
viewport placement, animation and outside-click dismissal. Menu rows align their
icons, labels and shortcut hints using CSS grid. File → From a
template opens a second anchored popup; View reflects its options with checkmarks.

A minimal popup looks like this:

```html
<button id="file">File</button>
<script>
  document.getElementById('file').addEventListener('click', function () {
    const panel = document.createElement('popup');
    panel.className = 'command-menu';
    panel.innerHTML = '<div role="menuitem" tabindex="0">New draft</div>';
    const handle = this.popup(panel, {
      anchorAt: 1, popupAt: 7,
      animationType: 'blend', animationDuration: 100
    });
    if (handle) handle.on('close', () => panel.remove());
    else panel.remove();
  });
</script>
```

```css
.command-menu {
  width: 260px;
  padding: 8px;
  background: white;
  border: 1px solid #d4e0d7;
  border-radius: 8px;
  box-shadow: 0 8px 28px #263e3224;
  popup-position: top-left bottom-left;
}
```

This snippet only opens a panel; add command handlers and keyboard behavior as
shown in the complete example. The points use numeric-keypad positions: anchor
1 is bottom-left, popup 7 is top-left. `popup-position` can supply these reference
points through CSS; the example's top-level menus use that CSS property. A submenu
uses anchor 9 (top-right) and popup 7. Keep a popup handle to close it explicitly.
Supplied DOM elements are not automatically removed on close; clean up ephemeral
panels in the handle's close callback.

The example supports hover switching, arrow keys, Home/End, Enter/Space, nested
Escape dismissal, outside-click closing, and Tab returning to normal focus order.
Disabled Export is skipped by keyboard navigation. Menu roles, aria-expanded and
aria-checked describe the state; a platform accessibility bridge is still pending.

Edit commands operate on the document's saved selection. The toolbar prevents
mousedown focus changes, and menu commands restore a cloned Range before calling
execCommand. Save draft/Ctrl/Cmd+S persists its HTML in localStorage; New draft and
templates replace the current document. Unsaved changes are lost on reload or
switching templates; PDF export is explicitly disabled. The View menu toggles the
sidebar, details panel and compact reading layout. Right-click the document for
an Edit context menu. Close window requests a native host action.

Popups remain inside the host window. Embedded frames have additional overlay
limits described in [composition](composition.md#current-limits). Native global
OS menus still require integration in the embedding host.

## Native dialogs, message boxes and popups

`Window.this` (shapes from docs.sciter.com/docs/DOM/Window) opens the system's own dialogs. They
block until answered and return `null` when cancelled, or when the host gave the page no dialogs
(headless renders, C hosts that haven't wired them):

```js
const url = Window.this.selectFile({ mode: "open", filter: "Text (*.txt)|*.txt;*.md|All (*.*)|*.*", caption: "Open" });
const urls = Window.this.selectFile({ mode: "open-multiple" });          // array of file:// URLs
const save = Window.this.selectFile({ mode: "save", extension: "txt" });
const dir = Window.this.selectFolder({ caption: "Pick a folder", path: "/tmp" });
Window.this.modal(<alert>Careful!</alert>);              // <info>, <alert>, <error>, <question>
if (Window.this.modal(<question>Delete it?</question>) === "yes") { /* ... */ }
```

`<question>` returns `"yes"` or `"no"` (Sciter doesn't document its return value). The system box is the
default and only shows the tag's plain text and a `caption`/`title`. To go further, opt in per box with an
`inline` attribute, or for every box with `Window.this.messageBoxes = "inline"` (`native` on a tag wins
over the setting). The box is then plain HTML in the window, `.nile-modal` > `.nile-messagebox.<kind>`
with `-title`, `-body`, `-icon`, `-text` and `-buttons` parts, so CSS restyles it; the tag's content
may be rich markup, and `<button value="save" default>` / `cancel` children replace the default OK or
Yes/No buttons. Enter chooses the default button, Escape the `cancel` one. Because it can't block the
script it returns a **Promise** of the clicked button's `value` (else its `id`, else its text):
`const r = await Window.this.modal(<question inline>…</question>)`. `new Window(...)`
and `modal({url})` for a second document aren't supported; use a `<frame>` or a popup.

`element.popup(menuOrJSX, { anchorAt, popupAt, x, y })` shows an element next to `element` (placement
points are numpad digits: 1 bottom-left … 9 top-right; the default is below the anchor), closes on a
click outside, on Escape, and after a menu item is clicked, and fires `popup-request`, `popup-ready`,
`popup-dismissing` and `popup-dismissed`. `element.takeOff({ x, y, width, height, relativeTo })` lifts
an element out of the flow until `takeOff()` is called without arguments. Right-clicking fires
`contextmenu`; set `evt.source = menuElement` (for example a `<menu.context>`) to show it at the
pointer. Placement and animation can come from CSS instead of the call: `popup-position: <popup point> <anchor point>`
(`top-left` … `bottom-right`, or `popup-reference-point` / `popup-anchor-reference-point`) and
`popup-animation: popup-animation(type: slide, axis: vertical, heading: start-to-end, duration: 150ms)`
(`blend`, `inflate`, `slide`, `roll`), read from the popup element and then its anchor; the call's
parameters (`anchorAt`, `popupAt`, `animationType`…) win. Hovering an element for half a second shows
its `title` (text) or `tooltip` (HTML) as a `popup[role=tooltip]`; a script can answer the bubbling
`tooltiprequest` event with `evt.source = element` to supply its own. Popups are painted inside the window, not as separate OS windows. See
[popups example](../../examples/popups.html) and [dialogs example](../../examples/dialogs.html).

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
