# Layout and styling

[Guide index](README.md) · Next: [Scripting](scripting.md)

## Dark mode, system colours and the accent colour

Nile follows the OS's theme. `@media (prefers-color-scheme: dark)` and `matchMedia` are true on a
dark system, and the system colour keywords (`Canvas`, `CanvasText`, `ButtonFace`, `ButtonText`,
`ButtonBorder`, `Field`, `FieldText`, `GrayText`, `LinkText`, `VisitedText`, `Highlight`,
`HighlightText`, `AccentColor`, `AccentColorText`, `Mark`) change with it. A page that says it
supports dark gets the platform's dark look for everything it doesn't style itself:

```html
<meta name="color-scheme" content="light dark">   <!-- or  :root { color-scheme: light dark } -->
```

Without that opt-in a page stays light, as in a browser; `color-scheme: dark` makes it dark even on a
light system. `color-scheme` also works on any element (it is inherited): inside it the user-agent
look of controls and the system colour keywords follow that element's scheme, so a dark sidebar in a
light app gets dark inputs and buttons (`examples/color-scheme.html`). Plain properties such as
`background` aren't changed by it, as in a browser; menus, scroll thumbs and the other things Nile
draws itself follow the page's (root) scheme. `matchMedia('(prefers-color-scheme: dark)')` lists fire
`change` (a `MediaQueryListEvent`) when the system flips, and `(min-width: ...)` lists when the window
is resized.
The system accent colours checked boxes and radios, focus rings, menu and list highlights,
progress bars, ranges and the text selection; `accent-color` (inherited) overrides it for a control
or a subtree. Test other themes with `NILE_COLOR_SCHEME=dark nile page.html` and
`NILE_ACCENT=#ff5500` (also for `--png`), and other platforms' controls with
`NILE_THEME=windows|macos|linux`. `examples/dark-mode.html` shows it. Embedders call
`nile_style::set_system_theme` (C: `nile_view_set_system_theme`) with what the OS says.

## Style sheets in files

Besides `<style>`, a page can link style sheets: `<link rel="stylesheet" href="app.css">` (with an optional
`media="(min-width: 700px)"`), and a sheet can `@import "other.css";` or `@import url(other.css) screen;`
(paths relative to the importing sheet; nested up to 8 deep). They are read when the page loads, from disk or
from a packed app, and spliced in as `<style>` text, so a sheet added later by script isn't loaded and
`url()` references inside a sheet resolve against the page's folder, not the sheet's. See
[css-file](../../examples/css-file/index.html).

## Start with predictable sizing

```css
* { box-sizing: border-box }
body {
  margin: 0;
  font-family: sans-serif;
  font-size: 14px;
  color: #263b46;
  background: #f3f6f7;
}
```

The default box model is `content-box`. With `border-box`, a declared width
includes padding and borders. Set `body` margin explicitly instead of relying on
the built-in user-agent stylesheet.

## Build an application shell

For a sidebar beside a scrolling main area:

```html
<div class="app">
  <aside class="sidebar">Navigation</aside>
  <main class="content">Your application content</main>
</div>
```

```css
.app { display: flex; height: 100vh }
.sidebar { width: 200px; flex-shrink: 0; padding: 20px }
.content { flex: 1; min-width: 0; overflow: auto; padding: 24px }
```

`min-width: 0` helps a flex child shrink when its content is wide. A scroller needs
a constrained size; otherwise it grows with its content. `overflow: auto` enables
scrolling and draggable overlay scrollbars, while `overflow: hidden` clips.

For cards or a calendar, use grid:

```css
.cards {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
}
.card { background: white; padding: 20px; border-radius: 8px }
@media (max-width: 900px) {
  .cards { grid-template-columns: repeat(2, 1fr) }
}
```

See [grid.html](../../examples/grid.html) for spans, template areas and automatic
tracks. HTML tables also work; see [tables.html](../../examples/tables.html).
They use grid-based layout. `table-layout: fixed` uses explicit table/column
widths; auto layout sizes columns from content. See
[table-fixed.html](../../examples/table-fixed.html) for column sizing examples.

## Reuse colors and interactive states

```css
:root { --accent: #19796a; --accent-hover: #126254 }
button { background: var(--accent); color: white; cursor: pointer }
button:hover { background: var(--accent-hover) }
button:active { opacity: 0.8 }
button:focus { border: 2px solid #73baa7 }
button:disabled { color: #96aaa2; cursor: default }
.hidden { display: none }
```

The shell supplies hover, active and focus states. `classList` lets scripts change
application states. Custom properties inherit and support `var()` fallbacks.
Unsupported CSS declarations are generally ignored; unsupported selectors can
cause a rule to be dropped. Check an example before assuming browser compatibility.
`:has()` is now supported. Border radius accepts one to four values and per-corner
longhands; elliptical radii are currently treated as circular. Inset box shadows
also work; see [radius.html](../../examples/radius.html) and
[inset-shadow.html](../../examples/inset-shadow.html).

## Images and text

Use local PNG/JPEG/WebP/GIF images or `data:` URLs. Relative image paths and CSS
`url()` paths resolve against the page's folder. Background images support size,
position and repeat. Remote HTTP images, SVG rendering, `object-fit` and painted
image alt text are not implemented.

Text is shaped using system fonts with fallback. Use a generic family or a font
installed on the target machine. Symbol glyphs may need an explicit family list:

```css
.symbol {
  font-family: "Apple Symbols", "Segoe UI Symbol", "Noto Sans Symbols", sans-serif;
}
```

Keep symbol testing cross-platform. Text wrapping, alignment, weight, italic,
decoration and letter spacing work. `white-space: pre`, `pre-wrap` and `pre-line`
preserve whitespace according to their modes, with tab stops. Use `<textarea>`
for a plain-text editable buffer or `contenteditable` for
[rich-text editing](rich-text-editing.md).

## Animation and overlays

Transforms, opacity, gradients, transitions and keyframe animations are available:

```css
.card { transition: background-color 150ms ease, transform 150ms ease }
.card:hover { background-color: #e3f1eb; transform: translateY(-2px) }
```

Listen for `transitionend`, `animationstart` and `animationend` to coordinate
application state. `transitioncancel` and `animationiteration` are not implemented.
See [anim-events.html](../../examples/anim-events.html). 3D transforms and perspective
project onto the page. `backface-visibility: hidden` hides a face while its composed
transform turns it away ([flip-card.html](../../examples/flip-card.html)); there is no
depth sorting, so do not expect a full 3D scene graph.

An absolute child is positioned against its nearest positioned ancestor. Use
`position: relative` on a menu's container and `position: absolute` on its popup.
`fixed` is pinned to the viewport; `sticky` follows page scrolling within its
parent, with vertical insets. Sticky positioning inside an overflow scroller is
not implemented. Stacking contexts support positioned boxes, opacity, transforms
and flex/grid items. Keep popups outside clipping ancestors: positioned descendants
of overflow/opacity/transform/sticky ancestors still paint with those ancestors.
See [fixed.html](../../examples/fixed.html) and [stacking.html](../../examples/stacking.html).
Frames have additional overlay limits covered in [composition](composition.md).

Whenever you change layout or styling, [render a PNG](cli.md) and inspect it at
the intended window sizes. Passing JavaScript checks does not verify visual layout.

Related reference: [CSS property and compatibility reference](../reference/css.md).

## Style scrollbars

Scroll containers (`overflow: auto | scroll`) and the page get thin overlay scrollbars that
grow and darken under the pointer. Restyle them per container (or on `html` for the page):

```css
.list { scrollbar-width: thin; scrollbar-color: #2f80ed #e3ecfb }   /* thumb, track */
.log  { scrollbar-width: none }                                     /* hidden, still scrolls */
.dark::-webkit-scrollbar { width: 12px }
.dark::-webkit-scrollbar-track { background: #1f2937 }
.dark::-webkit-scrollbar-thumb { background: #f59e0b; border-radius: 3px }
```

`::-webkit-scrollbar*` wins over `scrollbar-width`/`scrollbar-color`. See
[scrollbars.html](../../examples/scrollbars.html). There are no arrow buttons, idle fade-out
or `:hover` rules on the pseudo-elements yet.
