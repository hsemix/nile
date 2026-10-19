# Draw with `<canvas>`

`<canvas>` gives a page a bitmap it draws on with the Canvas 2D API. Nile keeps the bitmap in a Skia
raster surface (the crate `nile-canvas`) and paints it like an image, so a canvas costs no layout when
it changes: drawing only repaints.

```html
<canvas id="c" width="240" height="120"></canvas>
<script>
  const ctx = document.getElementById('c').getContext('2d');
  ctx.fillStyle = '#4285f4';
  ctx.fillRect(10, 10, 100, 60);
  const g = ctx.createLinearGradient(0, 0, 240, 0);
  g.addColorStop(0, '#ff6b6b'); g.addColorStop(1, '#48dbfb');
  ctx.strokeStyle = g; ctx.lineWidth = 6; ctx.beginPath();
  ctx.moveTo(10, 100); ctx.quadraticCurveTo(120, 60, 230, 100); ctx.stroke();
</script>
```

The box is `width` x `height` CSS pixels (300 x 150 by default) unless CSS sizes it; the bitmap is
stretched over it. Set `width`/`height` to the pixels you want on a high-density display
(`canvas.width = 240 * devicePixelRatio`) and give CSS the logical size. Assigning `width` or `height`
clears the canvas and resets its context, as in a browser. `examples/canvas.html` shows most of this.

## What is supported

- **State:** `save`, `restore`, `reset`, `scale`, `rotate`, `translate`, `transform`, `setTransform`,
  `resetTransform`, `getTransform`.
- **Styles:** `fillStyle`/`strokeStyle` (any CSS colour, `CanvasGradient` from `createLinearGradient`,
  `createRadialGradient` and `createConicGradient`, `CanvasPattern` from `createPattern`),
  `globalAlpha`, `globalCompositeOperation` (all the Porter-Duff and blend modes), `lineWidth`,
  `lineCap`, `lineJoin`, `miterLimit`, `setLineDash`/`getLineDash`, `lineDashOffset`, `shadowBlur`,
  `shadowColor`, `shadowOffsetX`/`Y`, `imageSmoothingEnabled`.
- **Rectangles and paths:** `clearRect`, `fillRect`, `strokeRect`, `beginPath`, `closePath`, `moveTo`,
  `lineTo`, `quadraticCurveTo`, `bezierCurveTo`, `arc`, `arcTo`, `ellipse`, `rect`, `roundRect`, `fill`
  and `clip` (`'nonzero'` or `'evenodd'`), `stroke`, `isPointInPath`, `isPointInStroke`, and `Path2D`
  (also from an SVG path string: `new Path2D('M10 10 L50 50 Z')`, `addPath`).
- **Text:** `font` (style, weight, size in px/pt/em, a family list), `textAlign`, `textBaseline`,
  `fillText`, `strokeText` (with a maximum width) and `measureText` (width and bounding boxes).
- **Images and pixels:** `drawImage` (an `<img>`, a `new Image()` once loaded, or another canvas; all three
  argument forms), `createImageData`, `getImageData`, `putImageData` (with a dirty rectangle),
  `canvas.toDataURL('image/png' | 'image/jpeg' | 'image/webp', quality)`, `canvas.toBlob(callback)`,
  `new OffscreenCanvas(w, h)` (an ordinary detached `<canvas>`).
- `requestAnimationFrame` drives animation; each frame you draw repaints the canvas.

## Differences from a browser

- Text uses Skia's fonts by family name (generic names such as `sans-serif`, `monospace` and `system-ui`
  map to each platform's fonts), not the page's `@font-face` fonts; `font-variant`, `letterSpacing` and
  `direction` are stored but not applied, and `filter` is ignored.
- A path's points are transformed when they are added (as the spec says) but a `Path2D` is drawn with the
  transform in force when it is filled.
- No `getContext('webgl')`, `ImageBitmap`/`createImageBitmap`, `captureStream` or `willReadFrequently`:
  `getContext` returns `null` for anything but `'2d'`.
- A canvas keeps its bitmap in memory until the page is reloaded (there is no garbage collection of
  surfaces yet), up to 16384 x 16384 pixels each.
- `<img>` gained `naturalWidth`, `naturalHeight`, `complete` and `src` events (`load`/`error` fire
  asynchronously), and `new Image()` works, so `img.onload = () => ctx.drawImage(img, 0, 0)` does too.
