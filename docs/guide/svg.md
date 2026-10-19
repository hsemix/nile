# Use SVG

Nile draws SVG with [resvg](https://github.com/linebender/resvg) (the crate `nile-svg`) in three places:

```html
<!-- inline: part of the page, coloured by CSS -->
<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
  <circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>
</svg>

<img src="logo.svg" width="64" height="64">

<div style="background: url(logo.svg) center / contain no-repeat"></div>
```

An SVG is rasterised at the size it is drawn at, on the display's pixel density, so it stays sharp when
scaled or on a 2x screen. `examples/svg.html` shows all three.

## Inline `<svg>`

- The element is a box like an `<img>`: `width`/`height` attributes or CSS size it; without them its
  `viewBox` is its size, else 300 x 150. `preserveAspectRatio` (including `none`) is honoured.
- `currentColor` is the element's CSS `color`, so `.icon { color: tomato }` and `:hover` recolour icons.
- Page style rules reach the elements inside it: `.themed .hot { fill: orange }` works even though
  `.themed` is outside the SVG. The page's selectors are matched against the real DOM; between page rules
  the later one wins (specificity isn't compared), `@media` blocks are skipped and a rule's declarations are
  used as written (no `var()`). A `<style>` inside the SVG and presentation attributes work as usual.
- Scripts change it like any element (`setAttribute`, `createElementNS`, `appendChild`); each change
  re-renders the picture.
- The children are not boxes: they have no layout rectangles, don't take pointer events of their own (a
  click on any part targets the `<svg>`), and `getBBox` doesn't exist.

## Files and backgrounds

`<img src="x.svg">`, `src="data:image/svg+xml,..."` and CSS `background-image: url(x.svg)` (with
`background-size`, `background-position` and `background-repeat`) use the file's `width`/`height`, else its
`viewBox`, as the natural size. `currentColor` is black in files, as in a browser.

## Limits

- No raster `<image>` elements inside an SVG, no SMIL animation (`<animate>`), no scripts, no
  `foreignObject`.
- SVG `<text>` uses the system's fonts (not `@font-face`) and costs about 0.7 MB of the binary. Embedders
  that don't want it build `nile-engine` with `default-features = false` (shapes and paths still draw).
- Fonts are scanned once per run, the first time an SVG with text is drawn.
