# Accessibility

Nile exposes every page to screen readers and other assistive technology (AT) through the platform's own
accessibility API: UI Automation on Windows, NSAccessibility (VoiceOver) on macOS and AT-SPI (Orca) on
Linux. The `nile` shell does this with [AccessKit](https://accesskit.dev/); other hosts get the same tree
as JSON from the C API. Nothing is built until assistive technology connects, so pages that nobody
inspects pay nothing.

## What the tree contains

The engine builds it from the DOM the way a browser does:

- **Roles** come from the element (`<button>`, `<a href>`, `<h1>`–`<h6>`, `<input type=…>`, `<select>`,
  `<table>`, `<ul>`, `<nav>`, `<main>`, `<details>`, `<progress>` ...) and are overridden by a `role`
  attribute (`role="tab"`, `"menuitem"`, `"dialog"`, `"alert"`, ... ). `role="presentation"`/`"none"` drops
  an element's own role. The window chrome roles (`window-close`, ...) are buttons named Close, Minimize
  and Maximize.
- **Names** come from `aria-label`, `aria-labelledby`, a `<label>` (wrapping or `for=`), `alt` on images,
  a `<fieldset>`'s `<legend>`, `title`, `placeholder`, and the element's own text for buttons, links,
  headings, cells and options. `aria-describedby` / `aria-description` give descriptions.
- **State and values**: disabled, checked (`aria-checked` for custom controls), expanded (`<details open>`,
  `aria-expanded`), selected, required, read-only, input values (passwords are masked), numeric ranges of
  sliders, spin buttons, progress bars and meters, heading levels and link targets.
- **Left out**: `display: none`, `hidden`, `aria-hidden="true"`, scripts and styles. Empty wrapper elements
  disappear; their text and controls move up to the parent.
- **Frames** appear as an `Iframe` node with the frame's own document below it.
- **Bounds** are in window coordinates and follow scrolling.

So the way to make a custom control accessible is the way you would on the web: use the native element,
or add `role`, `aria-label`, `aria-checked`, `aria-expanded` and `tabindex="0"` to your own.

## Live regions

`aria-live="polite"` or `"assertive"` marks a region whose changes are announced without moving focus;
`role="alert"` (assertive), `role="status"` and `role="log"` (polite) are live by default, and
`aria-atomic="true"` announces the whole region instead of just what changed (the default for `alert`
and `status`). A live region stays in the tree even when empty so the first message is spoken. The
tree carries the setting (`live`, `atomic` in the JSON) and AccessKit raises the platform's live-region
event when text inside changes. `examples/live-regions.html` has three buttons to try with a screen reader.

## What assistive technology can do

Click (buttons, links, checkboxes, options, summaries), focus, replace an input's text, step sliders and
spin buttons, and scroll an element into view. Focus follows the page's focus: when keyboard focus moves,
the screen reader hears it.

## Embedding

```c
char *json = nile_view_accessibility_tree(view);    // free with nile_string_free
bool changed = nile_view_accessibility_action(view, node_id, NILE_A11Y_ACTION_CLICK, NULL);
```

Build your platform nodes from the JSON, keep node ids as the ids you were given, and call
`nile_view_accessibility_tree` again when `nile_view_tick` / an input call reports the page changed.
Window coordinates are logical pixels.

## Limits

The tree carries no text-selection or caret positions inside
text (screen readers read inputs by value), there is no `aria-owns`/`aria-controls`, and the whole tree is
rebuilt on each change (fine for ordinary documents; a 1,500-row table costs a few milliseconds per
update). Popups shown in their own OS windows and secondary windows each have their own tree. I could
check the structure with AccessKit's consumer in tests but not with a real screen reader here, so treat
reports from VoiceOver, NVDA and Orca as the source of truth.
