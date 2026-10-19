# Drag and drop

Nile implements the HTML5 drag and drop events inside a page, and delivers files dropped from the
desktop through the same events.

## Dragging elements

Mark an element `draggable="true"`. Press on it (or on something inside it) and move the pointer a few pixels:
the drag starts, and a translucent copy of the element follows the pointer.

```js
card.addEventListener("dragstart", (e) => {
  e.dataTransfer.setData("text/plain", card.id);
  e.dataTransfer.effectAllowed = "move";   // none | copy | move | link | copyMove | copyLink | linkMove | all
});

zone.addEventListener("dragover", (e) => e.preventDefault());       // accepting the drop
zone.addEventListener("drop", (e) => {
  e.preventDefault();
  const id = e.dataTransfer.getData("text/plain");
  zone.append(document.getElementById(id));
});
card.addEventListener("dragend", (e) => console.log(e.dataTransfer.dropEffect));  // "none" if it wasn't accepted
```

| Event | Fires on | Notes |
| --- | --- | --- |
| `dragstart` | the dragged element | cancel it to stop the drag |
| `drag` | the dragged element | on every pointer move |
| `dragenter`, `dragover`, `dragleave` | what is under the pointer | `dragover` repeats; its `preventDefault()` accepts a drop there |
| `drop` | the accepting element | `dataTransfer` is readable now |
| `dragend` | the dragged element | `dataTransfer.dropEffect` says what happened |

The pointer shows the move cursor over an accepting element and the not-allowed cursor elsewhere. Escape cancels the
drag (`dragend` with the effect `none`). `getData()` only returns data in `drop`, as in browsers; `dragover` sees the `types`.
`DataTransfer` has `setData`, `getData`, `clearData`, `types`, `items`, `files`, `effectAllowed`, `dropEffect` and
`setDragImage` (accepted, but the drag image is always a snapshot of the element).

## Files from the desktop

Files dropped on the window fire `dragenter`, `dragover` and `drop` on the element under the pointer, with
`e.dataTransfer.files` (`File` objects with a `name`, `type`, `size` and their contents). Dropping on an
`<input type=file>` or its label still fills the input.

```js
zone.addEventListener("dragover", (e) => e.preventDefault());
zone.addEventListener("drop", async (e) => {
  e.preventDefault();
  for (const file of e.dataTransfer.files) console.log(file.name, (await file.text()).length);
});
```

## Not there yet

Dragging text selections, links and images by default (add `draggable="true"` yourself), dragging *out* of the
window to another application, dropping data from another application other than files, and the modifier keys
(Ctrl for copy) changing the effect.

Try it: `examples/drag-drop.html` (`nile examples/drag-drop.html --png out.png --drag 100,100,360,130` renders a drag in
progress).
