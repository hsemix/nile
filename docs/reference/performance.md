# Scrolling and large interfaces

[Reference index](README.md)

## Keep desktop chrome outside the scroller

A constrained scroll region prevents a long list from moving the whole app.

```css
html, body { height:100%; margin:0 }
body { position:fixed; inset:0; display:grid;
       grid-template-rows:auto minmax(0,1fr) auto; overflow:hidden }
.workspace { min-width:0; min-height:0; overflow:hidden }
.viewport { height:100%; min-height:0; overflow:auto }
```

For nested grids/framesets, constrain the row with `minmax(0,1fr)` as well.
Flex/grid items can otherwise retain intrinsic content size and expand the page.
A height declaration alone does not always constrain all descendants.

Wheel input scrolls the innermost eligible container, then chains to ancestors
and the page at boundaries. Native trackpad pixel deltas are preserved in logical
pixels; mouse wheel lines use the shell's fixed scaling. No browser smooth-scroll
animation is built into `scrollTo`. Scroll offsets affect hit testing and paint;
ordinary scrolling can reuse geometry. Native select lists have additional
scroll-dependent data. Do not mutate every row just because a scroll event fired.

## Virtual lists

Fixed-height lists need only a scroll viewport, a full-height spacer and a small
positioned slice. Store item data/selection outside the DOM:

```js
const rowHeight = 36, overscan = 3;
function visibleRange(viewport, count) {
  const first = Math.max(0, Math.floor(viewport.scrollTop / rowHeight) - overscan);
  const end = Math.min(count,
    Math.ceil((viewport.scrollTop + viewport.clientHeight) / rowHeight) + overscan);
  return {first,end}; // end is exclusive
}
```

Set spacer height to `count * rowHeight`; position the slice at
`first * rowHeight`. Render `items[first..end]`. Selection uses logical row IDs,
not mounted elements. Keyboard navigation should update selection and explicitly
scroll to it. Native `scrollIntoView` is partial; computing offsets is predictable.

## Two-axis virtual grids

[Virtual grid](../../examples/virtual-grid/index.html) represents up to one
million rows × 100 columns, using generated values and sparse edits. It does not
allocate a 100-million-cell array.

```js
const row = Math.floor(viewport.scrollTop / 36);
const column = Math.floor(viewport.scrollLeft / 140);
```

Apply the same visible-range calculation on both axes. Spacer dimensions are
rowCount×rowHeight and columnCount×columnWidth. Position the rendered rectangle
at its logical top/left, and move a separate header with horizontal scrolling.
Overscan reduces blank edges during scheduled updates.

At the example's initial 1280×820 viewport around 190 cell elements are mounted;
this varies with size and overscan. A large logical range can still have scroll
precision limits because engine coordinates are floating-point values. This is
not an unlimited-size claim.

Use `requestAnimationFrame` to coalesce events:

```js
let pending = false;
function schedule() {
  if (pending) return;
  pending = true;
  requestAnimationFrame(() => { pending = false; updateVisibleCells(); });
}
viewport.addEventListener('scroll', schedule);
new ResizeObserver(schedule).observe(viewport);
```

Only rebuild when index bounds change. Keep editors separate or commit before
recycling their cells. The example preserves edits in a sparse map, supports
keyboard navigation and clamps ranges at both ends. See its
[implementation walkthrough](../../examples/virtual-grid/README.md).

Variable heights/widths require measured dimensions and cumulative offsets;
fixed tracks make this version simpler. Sorting/filtering should operate on the
data model. Load large data in pages; rendering a small DOM does not make a large
synchronous sort, file read or database query inexpensive.

## Avoid repeated synchronous work

- Batch DOM writes before geometry reads; measurement can force sync/layout.
- Keep event handlers short. Timers/promises do not automatically move work off
  the UI thread; `@sys` metadata and whole-file promises currently wrap sync I/O.
- Delegate listeners instead of attaching new handlers during every render.
- Preserve controls and ranges when focus matters; prefer keyed patching or
  updating existing elements over replacing complete forms.
- Detached nodes remain in the current DOM arena. Virtualization bounds live
  elements, but repeated slice replacement is not a proof of constant lifetime
  memory. Pool/reuse nodes when targeting sustained scrolling workloads.
- For expensive native work, expose host services that queue work and return
  results through the app's messaging/tick lifecycle.

## Measure the actual workload

```sh
NILE_TRACE=1 NILE_STORAGE_DIR=off cargo run -p nile-shell -- \
  examples/virtual-grid/index.html --png /tmp/grid.png --wheel 500,400,-300 --settle 100
cargo test -p nile-engine bench_scroll -- --ignored --nocapture
```

Trace logs separate style/layout/paint. One headless snapshot includes startup,
font setup and a limited input sequence; it is not a sustained frame-rate or
trackpad-latency benchmark. Test warm frames at the actual physical pixel scale,
including hover, accessibility, editing and data updates. Dev dependencies are
optimized in this workspace; release measurements still matter.

The shell paints through Skia onto a CPU-backed surface. Larger/retina windows
increase pixel cost. There is no established universal performance ratio against
Electron; benchmark equivalent workloads rather than infer speed from binary size.
