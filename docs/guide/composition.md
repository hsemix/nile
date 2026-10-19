# Includes, frames and resizable panes

[Guide index](README.md) · Related: [Layout](layout-and-style.md)

## Include shared markup in one document

```html
<head>
  <include src="include/styles.htm"></include>
</head>
<body>
  <include src="include/header.htm"></include>
  <include src="include/nav.htm"><p>Navigation unavailable.</p></include>
</body>
```

The loader splices HTML fragments into the parent document. Included elements
share its DOM, styling and script context. An included `<style>` supplies shared
CSS; an include is not an external stylesheet link. Includes are processed while
loading, rather than acting as a reactive component when their attributes change.
Failed loading or a false `media` condition keeps the include's fallback content.
Top-level include paths resolve from the page folder; nested include paths
resolve from the fragment containing them. Expansion stops after 16 levels to
limit nesting and cycles. Included markup then belongs to the outer document,
so its ordinary image/script URLs use the outer page's asset base.

## Platform-conditional elements

`media="platform != 'OSX'"` (also `==`, joined with `and` / `or`; platforms `Windows`, `OSX`,
`Linux`) works on **any element**, not only `<include>`: when the expression is false the element and
its subtree are left out when the page is parsed.

```html
<header media="platform != 'OSX'"><!-- custom caption buttons --></header>
```

This is a Nile extension. Sciter documents `media` only for `<include>`, `<link>`, `<style>` and
`<script>`, so a page that relies on it elsewhere shows the element in real Sciter. Unrecognised
expressions count as true, a script that sets `media` later has no effect, and `<link media="print">`
keeps its CSS meaning.

```sh
cargo run -p nile-shell -- examples/include.html
```

The example also shows platform media expressions, including
`media="platform != 'Nope'"`. Use an include for reusable headers or CSS that
should become part of the same document.

`<html>`'s own attributes can't be conditional this way (an `<include>` can't set attributes on its
parent either). For the window, add a platform suffix to any `window-*` attribute:
`<html window-frame="solid" window-frame-osx="native">` gives macOS a native frame and the other
platforms a solid one. Suffixes are `-osx`, `-windows` and `-linux`; also a Nile extension.

## Embed a separate document

```html
<frame id="content" src="frames/content.html" style="width:600px;height:300px"></frame>
```

`<frame>` and `<iframe>` each have a separate DOM, styles, runtime, scrolling and
input state, painted inside their layout box. Give the box a useful size. The
child's asset paths resolve from its own file. The outer document can access a
proxy for the frame document and exchange messages:

```js
const frame = document.getElementById('content');
frame.addEventListener('complete', () => console.log('Frame ready'));
frame.postMessage({ action: 'refresh' });
frame.loadHtml('<body><p>Replacement content</p></body>');
```

Load methods include `loadFile`, `loadHtml` (with an optional base URL) and
`loadEmpty`. `src` changes load a file. `frame.document` / `contentDocument`
expose proxy queries; `frame.contentWindow` provides messaging/evaluation.
These are proxy operations across a JSON boundary, not shared DOM objects.
The child uses `window.parent.postMessage(data)` to reply; the receiver listens
for `message`. Loading emits `newdocument` and `complete`; loading is synchronous,
so initial loading can precede listeners registered by the parent script.

`content-style="frames/theme.css"` adds a stylesheet to the child document after
its own styles. This frame-specific mechanism does not mean ordinary external
CSS links are loaded. See [frames.html](../../examples/frames.html).

## Resize panes with framesets

```html
<frameset id="panes" cols="140px,*,160px" style="height:400px">
  <frame src="frames/help-index.html"></frame>
  <splitter></splitter>
  <frame src="frames/content.html"></frame>
  <splitter></splitter>
  <div>A regular block can be a pane.</div>
</frameset>
```

`cols` lays out panes horizontally; `rows` lays them out vertically. Splitters
between panes let users resize adjacent panes with the pointer. Read or set
`document.getElementById('panes').frameset.state` for their current sizes.
Splitters are excluded from the pane list. Size the frameset explicitly or give
it constrained flex space, as in [frameset.html](../../examples/frameset.html).

```sh
cargo run -p nile-shell -- examples/frames.html
cargo run -p nile-shell -- examples/frameset.html
```

## Current limits

Frames paint above outer-page content, so an outer dropdown or fixed overlay
cannot cover a frame. Frame placement and input now follow ancestor overflow clips and transforms,
as demonstrated by [transformed frames](../../examples/frames-transformed.html).
Frames inside paged paper do not yet use the sheet mapping. Tab does not enter a frame; click inside to direct input
there. Frame loading is synchronous with no `:busy` state. Use includes or normal
DOM components instead when you need shared selection, integrated overlay layering
or one script context.
