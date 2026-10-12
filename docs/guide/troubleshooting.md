# Troubleshooting and compatibility

[Guide index](README.md)

## My stylesheet has no effect

Use `<style>` in the HTML or inject a style element from JavaScript. External
`<link rel="stylesheet">` loading is not implemented. Check the selector and the
[supported properties](../../README.md#whats-supported-today). Unsupported selector
syntax can drop a rule. For dynamic styles, use `.style` or `.classList`; avoid
assuming every browser CSS property is available.

## An image or imported script is missing

Run from the repository root when using the guide's commands. Asset paths resolve
from the page folder; module imports resolve from the importing file. An image
referenced by injected CSS still resolves from the page folder. Use local files
or supported data URLs for images. HTTP loading, bare npm imports and `this://`
resource URLs are not implemented. Check the terminal for script load errors.

## Text cannot be selected

Add `user-select: text` to the reading area, or a `selectable` attribute. Page-text
selection is deliberately opt-in. A text input has its own editable selection.
Page selection and copying are available; selecting a paragraph does not enable
rich-text editing. See [selection](controls-and-menus.md#enable-page-text-selection).

## A symbol appears as a box

Specify an appropriate symbol font family and check that it is installed on the
target platform. Generic fallback does not cover every symbol. Prefer CSS-drawn
simple icons if system-font differences are unacceptable. Loading custom web fonts
with `@font-face` is not implemented.

## A popup is clipped or appears behind content

Check ancestor `overflow` and positioning. Use a positioned parent for local
dropdowns, and put viewport overlays outside scrolling containers. Current
`z-index` support orders siblings and does not implement complete stacking
contexts. `position: fixed` does not pin content during page scrolling.

## Typing loses focus

Replacing an input or its ancestors with `innerHTML`/`content()` creates new
controls. Update only the results or labels during typing. If you must replace
the input, explicitly preserve and restore its value, focus and selection.

## Saving, fetching or navigation does not work

The development shell supplies the UI runtime, not application storage or network
services. The showcase's saves are in-memory snapshots, and its mail composer
does not send email. There is no built-in `fetch`, browser URL navigation or
document persistence. Register host functions through [the embedding API](embedding.md)
for application services; do not assume browser storage APIs.

## A snapshot exists, but the app did not behave as expected

Read terminal errors. The shell can emit a PNG after a JS exception. Use a known
completion marker in automated checks. Input coordinates are logical pixels,
even with `--scale 2`. Use `--wait` before simulated input and `--settle` after it.
The CLI stores one pointer action; repeated click flags do not replay multiple
clicks. Check whether your reading area opted into selection.

## The interface becomes slow as it grows

Keep DOM updates small. Do not recreate an entire page for a single label change.
Batch layout reads after writes. Paint-only style changes can reuse existing
layout, but some state changes still cascade styles across the document. Detached
nodes remain in the arena, so repeatedly rebuilding large trees can grow memory.
Use `NILE_TRACE=1` and [stress.html](../../examples/stress.html) to inspect phase costs.

## Packaging expectations

The development binary is in `target/debug`; use `cargo build -p nile-shell --release`
for `target/release/nile`. Ship the HTML, scripts and images together
with their relative paths intact. The shell has no installer or resource-bundle
packer. Fonts come from the target OS. For an app with its own executable and
services, embed `nile-engine` or `libnile` rather than treating the development
shell as a complete application framework.

This guide reflects the current checkout. Check the [known gaps](../../README.md#known-gaps-deliberate-for-now)
before relying on browser/Sciter behavior, especially IME, accessibility, rich-text
editing, GPU rendering and full CSS compatibility.
