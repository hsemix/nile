# Your first Nile app

[Guide index](README.md) · Next: [Layout and style](layout-and-style.md)

## Build the development shell

Install a recent stable Rust toolchain with Cargo. On macOS, install the Xcode
command-line tools if they are missing:

```sh
xcode-select --install
```

From the repository root:

```sh
cargo build -p nile-shell
cargo run -p nile-shell -- examples/getting-started/index.html
```

The first build can take time: dependencies include Skia, and its build may
download prebuilt binaries. It needs network access. Windows and Linux builds
also need the platform's native compiler/linker; dependency build output identifies
any missing tools or system libraries.

Enter a name, click **Say hello**, then try **Toggle theme**. Drag over the greeting
and use Ctrl+C or Cmd+C to copy it. Focus the name input and press Enter to greet.

Reload after editing with **F5** or **Ctrl/Cmd+R**. Reloading recreates the document
and JavaScript runtime, so in-memory state resets. A plain `R` is typed as text;
it is not a reload shortcut.

## What makes the app work

The complete runnable source is [index.html](../../examples/getting-started/index.html).
It contains three parts:

- HTML defines the controls and content.
- A `<style>` element defines their appearance and layout.
- A `<script>` element handles clicks and updates the DOM.

Its event handler follows the same pattern you will use throughout an app:

```js
const name = document.getElementById('name');
const result = document.getElementById('result');

function greet() {
  result.textContent = 'Hello, ' + (name.value.trim() || 'friend') + '!';
}

document.getElementById('greet').addEventListener('click', greet);
```

Changing the DOM causes Nile to update the interface. You do not need to call a
render method from page JavaScript. Use `textContent` for plain text, particularly
values typed by a user; `innerHTML` parses markup.

## Create your own page

Copy the example into a folder such as `examples/my-app/`, keeping the filename
`index.html`, then run:

```sh
cargo run -p nile-shell -- examples/my-app/index.html
```

Keep asset paths relative to the HTML file. For example, a page beside `app.js`
and an `images/` folder can use:

```html
<img src="images/logo.png" width="48" height="48">
<script src="app.js"></script>
```

Nile currently does not load styles from `<link rel="stylesheet">`. Start with
`<style>` in the HTML. To share CSS across pages, export a CSS string from a JS
module and append a `<style>` element; the [workbench styles](../../examples/workbench/styles.js)
and [app](../../examples/workbench/app.js) demonstrate that approach.
For reusable markup and style fragments, `<include>` is also supported. For
separate document runtimes and resizable panes, see [composition](composition.md).

## Save a preview

```sh
cargo run -p nile-shell -- examples/getting-started/index.html --png /tmp/nile-first-app.png --width 720 --height 500 --scale 2
```

This produces a 1440 × 1000 PNG without opening a window. The viewport remains
720 × 500 logical pixels. See the [CLI guide](cli.md) to simulate typing and clicks.
