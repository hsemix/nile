# Layout and styling

[Guide index](README.md) · Next: [Scripting](scripting.md)

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
They use grid-based layout and have compatibility limits documented in the README.

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
decoration and letter spacing work; whitespace preservation for `pre`/`pre-wrap`
is incomplete. Use `<textarea>` for a plain-text editable buffer, and `<br>` for
explicit breaks in display text.

## Animation and overlays

Transforms, opacity, gradients, transitions and keyframe animations are available:

```css
.card { transition: background-color 150ms ease, transform 150ms ease }
.card:hover { background-color: #e3f1eb; transform: translateY(-2px) }
```

An absolute child is positioned against its nearest positioned ancestor. Use
`position: relative` on a menu's container and `position: absolute` on its popup.
Keep popups outside clipping scrollers when possible. `z-index` currently reorders
siblings rather than implementing complete browser stacking contexts. `fixed`
scrolls like an absolute element; `sticky` behaves like relative positioning.
These constraints matter for modal and menu placement.

Whenever you change layout or styling, [render a PNG](cli.md) and inspect it at
the intended window sizes. Passing JavaScript checks does not verify visual layout.
