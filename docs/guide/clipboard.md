# Clipboard

`Clipboard` follows Sciter's API (docs.sciter.com/docs/JS.runtime/Clipboard). It is a global and is
also exported by `@nile` / `@sciter`.

```js
Clipboard.writeText("hello");
Clipboard.write({ html: "<b>Bold</b>", text: "Bold" });
Clipboard.write({ image: canvas.getContext("2d").getImageData(0, 0, w, h) });

const c = Clipboard.read();          // { text?, html?, json?, file?, image? }
Clipboard.readText();                // string | undefined
Clipboard.has("image");              // "text" | "html" | "image" | "file" | "json"
```

| Flavour | Value | Notes |
|---|---|---|
| `text` | string | |
| `html` | string | Pair it with `text` as the plain form. |
| `image` | `ImageData` (RGBA) | Sciter has a `Graphics.Image`; Nile has none, so pictures are `ImageData`. Draw one with `putImageData`. |
| `file` | array of paths | What a file manager copied; writing puts the paths on the clipboard as files. |
| `json` | any JSON value | Not a system flavour: it is kept inside the app and counts only while the text written with it is still on the clipboard, so copy/paste between your own windows works and a later copy elsewhere hides it. |

Limits: a system clipboard holds one primary flavour at a time, so `write` uses the first of `image`,
`file`, `json`, `html`, `text` that is present (`text` rides along with `json` and `html`). `link` is not
supported. Nothing is synchronised with other processes except text, html, images and files. Headless
runs and tests use a private in-memory clipboard. C hosts that supply their own clipboard get the
text and HTML callbacks only; images and files are ignored there.

Example: [clipboard.html](../../examples/clipboard.html).
