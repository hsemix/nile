# CSS reference

[Reference index](README.md) · [Layout tutorial](../guide/layout-and-style.md)

## Optional platform theme

The [system theme package](../../themes/system/README.md) is loaded explicitly
with `<include src="themes/system/system.htm"></include>`. It selects Windows,
macOS or Linux sheets and styles supported controls and components, including
hover/pressed/focus states. It is not part of the engine and does not read live
OS accent/dark-mode settings. Renderer-defined internal control parts have
styling limits documented by the package.

## Style sources and values

Use `<style>`, `style="..."` or `<link rel="stylesheet" href="app.css">`.
External sheets and nested `@import` are loaded before parsing the document;
links inserted later by JavaScript are not automatically loaded. Import/include
recursion is limited to 16 levels for markup and 8 for CSS imports. Relative
`url()` resources in external CSS resolve against the page's asset base, not the
sheet's directory. Packed resources work through `this://app/`.

Lengths include `px`, `%`, `em`, `rem`, `vw`, `vh` and physical units such as
`in`, `cm`, `mm`, `pt`, `pc`, `q`. `fr` is a grid track unit. `calc()`, `min()`,
`max()` and `clamp()` use the implementation's length-expression support;
percentage resolution needs a definite containing size and is partial for mixed
expressions. Named colors, hex, RGB/RGBA and HSL/HSLA are supported. Modern color
spaces and `color-mix()` are not implemented.

`@media` supports width/height (including min/max and simple range comparisons),
orientation, screen/all, and/not/comma combinations, plus fixed light-theme,
no-preference reduced-motion, hover and fine-pointer values. Include `media`
platform expressions are a separate markup feature. `@supports` tests
property names, not value validity: `(display: nonsense)` can pass. These are
not complete browser feature-detection APIs. `@layer`, CSS nesting and
`@property` registration are not implemented. Tailwind-generated CSS therefore
needs a compatibility audit; it is not a drop-in dependency.

## Selectors and cascade

| Family | Usage |
|---|---|
| Basic selectors | `button`, `.primary`, `#save`, `*` |
| Attributes | `[disabled]`, `[data-id="42"]`, prefix/suffix/substring operators |
| Combinators | Descendant, child `>`, next sibling `+`, following siblings `~` |
| Structural | `:root`, `:empty`, `:first-child`, `:last-child`, `:nth-child()`, and related supported structural selectors |
| Logical | `:not()`, `:is()`, `:where()`, `:has()` |
| Interaction | `:hover`, `:active`, `:focus` driven by actual input |
| Runtime state | `:checked`, `:disabled`, `:readonly`, `:expanded`, `:collapsed`, `:current`, `:selected`, `:invalid`, `:busy` |
| Generated content | `::before`, `::after`; `::marker` has limited content/color/font support |

```css
:root { --accent: #7951be }
button.primary { background: var(--accent); color: white }
button:disabled { opacity: .45 }
.panel:has(input:focus) { border-color: var(--accent) }
.caption::before { content: attr(data-prefix); margin-right: 8px }
```

Custom properties inherit and resolve through `var(--name, fallback)`.
Specificity, inheritance and `!important` participate in the cascade. Runtime
flags are also available through `element.state`; changing a state does not
create the corresponding widget behavior. Unsupported pseudo-classes such as
`:visited` and unsupported pseudo-elements can invalidate the selector rule.

## Layout

| Family | Properties and behavior | Example |
|---|---|---|
| Box model | `box-sizing`, width/height and min/max, margins, padding, borders; default is content-box | [Hello](../../examples/hello.html) |
| Normal flow | Block, inline, inline-block, `display:none`; inline layout remains partial | [Inline](../../examples/inline.html) |
| Flex | Direction, wrapping, grow/shrink/basis, `flex`, alignment, gaps | [Hello](../../examples/hello.html) |
| Grid | Explicit/implicit tracks, line placement/spans, areas, auto flow, alignment, `repeat`, `minmax`, auto-fit/auto-fill | [Grid](../../examples/grid.html) |
| Tables | Semantic table elements, spans, spacing/collapse, vertical alignment | [Tables](../../examples/tables.html) |
| Fixed tables | `table-layout:fixed` with definite table width; `<col>` or first-row widths | [Fixed tables](../../examples/table-fixed.html) |
| Floats | `float`, `clear`; paragraph text wraps alongside floats line by line | [Float](../../examples/float.html) |
| Shapes | `shape-outside` circle/ellipse/inset/polygon/reference boxes and `shape-margin` | [Shapes](../../examples/shape-outside.html) |

```css
* { box-sizing: border-box }
.workspace { display: grid; grid-template-columns: 220px minmax(0,1fr); gap: 16px }
.content { min-width: 0; min-height: 0; overflow: auto }
.cards { display: grid; grid-template-columns: repeat(auto-fit,minmax(180px,1fr)); gap: 12px }
```

Constrain both dimensions when a child should scroll; use `min-height:0` and
`min-width:0` on shrinking grid/flex items. Framesets are special grids whose
`cols` or `rows` attributes set pane tracks; see [composition](../guide/composition.md).

Limits: no named grid lines or subgrid; `grid-template` shorthand is not
implemented. Automatic table layout sizes from content rather than honoring
column widths. Collapsed borders overlap without a browser's conflict rules;
caption placement and row-group backgrounds are partial. Inline-block
shrink-to-fit, wrapped inline decorations and block children inside inline
content remain limited. Image-based shape outlines are not implemented.

## Positioning and scrolling

`position:relative/absolute/fixed/sticky`, `top/right/bottom/left`, `inset`,
`z-index` and `overflow/overflow-x/overflow-y` are supported.
`pointer-events` is not currently dispatched by the cascade.
`hidden` clips and allows programmatic scrolling; `auto`/`scroll` also respond to
input and show draggable overlay bars. Bars are not CSS-styleable.

```css
html, body { height:100%; margin:0 }
body { position:fixed; inset:0; display:grid;
       grid-template-rows:auto minmax(0,1fr) auto; overflow:hidden }
.list { min-height:0; overflow:auto }
```

This pins desktop chrome while the middle pane scrolls. See
[scrolling APIs](dom.md#measurements-and-scrolling). Fixed elements use viewport
coordinates, but an ancestor clip can still apply. Sticky positioning is
vertical and follows page scrolling, not an element's scroll offset. Stacking
contexts cover position/z-index, opacity and transforms; lifting descendants
out of clipping/opacity ancestors has limitations. `isolation`, `will-change`
and `mix-blend-mode` do not create contexts. Rounded clips are painted, but
rounded-corner hit testing is not equivalent.

## Text and fonts

| Feature | Properties/markup |
|---|---|
| Font | `font-family`, `font-size`, `font-weight`, `font-style`, `font` shorthand |
| Text layout | `line-height`, `letter-spacing`, `text-align`, `text-transform` |
| Whitespace | `normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line` |
| Decoration | Underline and line-through through `text-decoration`/`text-decoration-line` |
| Overflow | Single clipped line with `text-overflow:ellipsis` |
| Lists | `list-style`, `list-style-type`, `list-style-position`, `<ol start reversed>`, `<li value>` |
| Inline alignment | `vertical-align`: sub/super/lengths, `<sub>` and `<sup>`; other inline modes are partial |
| Selection | `user-select`, `selectable` attribute, `behavior:htmlarea`, selection background properties |

System font shaping/fallback is provided through Parley/Skia. Explicit symbol
font families may still be necessary. Variable-font axes are not applied when
painting glyphs.

```css
@font-face { font-family: AppFont; src:url("fonts/Cantarell-Regular.ttf"); font-weight:400 }
body { font-family: AppFont, sans-serif }
.title { white-space:nowrap; overflow:hidden; text-overflow:ellipsis }
```

TrueType, OpenType, WOFF and WOFF2 sources can load from local files, data URLs
or packed assets. `local()`, `unicode-range` and `font-display` are ignored.
See [fonts](../../examples/font-face.html) and [markers](../../examples/markers.html).

## Painting

Background color/image, positioning, size and repeat are supported for a single
background layer. Images decode supported raster formats from local/packed
files or data URLs; remote `<img>` loading and SVG rendering are not implemented.
SVG text can be viewed as a file, but it is not an image format for the renderer.

Linear/radial gradients, repeating gradients, opacity, outer/inset box shadows,
borders and corner radii are supported. Radius supports percentages and
per-corner values; elliptical slash radii are read as circular. Some decorative
border styles paint as solid. `object-fit`, multiple backgrounds,
`background-origin`/`background-attachment`/`background-clip` are not implemented.

Examples: [images](../../examples/images.html), [gradients](../../examples/gradients.html),
[shadows](../../examples/inset-shadow.html), [radii](../../examples/radius.html).

## Transforms and motion

Use `transform`, `transform-origin`, `perspective`, `perspective-origin`,
transition properties and animation properties. Transform functions include
2D translate/scale/rotate/skew/matrix and projected 3D variants.

```css
.card { transition: background-color 150ms ease, transform 150ms ease }
.card:hover { background:#eee6fa; transform:translateY(-2px) }
@keyframes pulse { from { opacity:.5 } to { opacity:1 } }
.indicator { animation:pulse 800ms ease-in-out infinite alternate }
```

Timing includes ease variants, linear, cubic-bezier and steps. Iteration/cancel events are not emitted. Script events
cover `transitionend`, `animationstart` and `animationend` notifications; see
[animation example](../../examples/anim-events.html). Interpolation depends on
property support, not just the property's presence in a declaration.
3D projection affects paint and hit tests but is flattened: no preserve-3d,
backface visibility or depth sorting. See [2D](../../examples/transforms.html),
[3D](../../examples/transforms3d.html), [animations](../../examples/animations.html).

## Pagination

`@page`, break properties and `page-view:paged` create paginated screen views
and PDF output. Page sizes/margins, six margin boxes and page/page-count counters
are documented in [printing](../guide/printing.md). Legacy `page-break-*` aliases
work. This is an engine feature distinct from an application button labeled
Print; a demo button need not be connected to PDF export.

## Supported property name inventory

The following inventory is drawn from the cascade implementation. Recognition
of a property name does not mean every CSS value is supported. Grid, animation,
font shorthand and generated-content declarations have separate dispatch paths.

| Property | Property | Property |
|---|---|---|
| `-moz-user-select` | `-webkit-appearance` | `-webkit-user-select` |
| `align-content` | `align-items` | `align-self` |
| `animation` | `animation-delay` | `animation-direction` |
| `animation-duration` | `animation-fill-mode` | `animation-iteration-count` |
| `animation-name` | `animation-timing-function` | `appearance` |
| `background` | `background-color` | `background-image` |
| `background-position` | `background-repeat` | `background-size` |
| `behavior` | `border` | `border-bottom` |
| `border-bottom-color` | `border-bottom-left-radius` | `border-bottom-right-radius` |
| `border-bottom-style` | `border-bottom-width` | `border-collapse` |
| `border-color` | `border-left` | `border-left-color` |
| `border-left-style` | `border-left-width` | `border-radius` |
| `border-right` | `border-right-color` | `border-right-style` |
| `border-right-width` | `border-spacing` | `border-style` |
| `border-top` | `border-top-color` | `border-top-left-radius` |
| `border-top-right-radius` | `border-top-style` | `border-top-width` |
| `border-width` | `bottom` | `box-shadow` |
| `box-sizing` | `break-after` | `break-before` |
| `break-inside` | `clear` | `color` |
| `column-gap` | `content` | `cursor` |
| `display` | `flex` | `flex-basis` |
| `flex-direction` | `flex-grow` | `flex-shrink` |
| `flex-wrap` | `float` | `font` |
| `font-family` | `font-size` | `font-style` |
| `font-weight` | `gap` | `grid-area` |
| `grid-auto-columns` | `grid-auto-flow` | `grid-auto-rows` |
| `grid-column` | `grid-column-end` | `grid-column-gap` |
| `grid-column-start` | `grid-gap` | `grid-row` |
| `grid-row-end` | `grid-row-gap` | `grid-row-start` |
| `grid-template-areas` | `grid-template-columns` | `grid-template-rows` |
| `height` | `inset` | `justify-content` |
| `justify-items` | `justify-self` | `left` |
| `letter-spacing` | `line-height` | `list-style` |
| `list-style-position` | `list-style-type` | `margin` |
| `margin-bottom` | `margin-left` | `margin-right` |
| `margin-top` | `max-height` | `max-width` |
| `min-height` | `min-width` | `opacity` |
| `overflow` | `overflow-x` | `overflow-y` |
| `padding` | `padding-bottom` | `padding-left` |
| `padding-right` | `padding-top` | `page-break-after` |
| `page-break-before` | `page-break-inside` | `page-view` |
| `perspective` | `perspective-origin` | `place-content` |
| `place-items` | `place-self` | `popup-anchor-reference-point` |
| `popup-animation` | `popup-position` | `popup-reference-point` |
| `position` | `right` | `row-gap` |
| `shape-margin` | `shape-outside` | `table-layout` |
| `text-align` | `text-decoration` | `text-decoration-line` |
| `text-overflow` | `text-selection` | `text-selection-background-color` |
| `text-transform` | `top` | `transform` |
| `transform-origin` | `transition` | `transition-delay` |
| `transition-duration` | `transition-property` | `transition-timing-function` |
| `user-select` | `vertical-align` | `white-space` |
| `width` | `z-index` |  |

Source: [cascade](../../crates/nile-style/src/lib.rs), [grid](../../crates/nile-style/src/grid.rs), [animation](../../crates/nile-style/src/anim.rs).
