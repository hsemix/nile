# Shell and snapshot reference

[Guide index](README.md) · Next: [Troubleshooting](troubleshooting.md)

Build with `cargo build -p nile-shell`, then use `target/debug/nile` (on Windows,
`target\debug\nile.exe`). Alternatively use `cargo run -p nile-shell --` before
the same arguments.

```sh
cargo run -p nile-shell -- examples/hello.html
cargo run -p nile-shell -- examples/hello.html --png /tmp/nile-hello.png
```

Without `--png`, the shell opens a native window. Window size/frame/title can be
configured with the root HTML attributes described in [embedding](embedding.md).
Use F5 or Ctrl/Cmd+R to reload. The shell currently loads file paths, not HTTP URLs.

## Snapshot flags

| Flag | Effect |
|---|---|
| `--png PATH` | Render without a window and write a PNG |
| `--width N` | Logical viewport width; default 900 |
| `--height N` | Logical viewport height; default 640 |
| `--scale N` | Pixel scale; default 1. Use a positive finite value |
| `--hover X,Y` | Move the pointer to a logical coordinate |
| `--press X,Y` | Move and press the primary button, leaving it held |
| `--click X,Y` | Move, press and release the primary button |
| `--wheel X,Y,DY` | Wheel at a coordinate; negative DY scrolls further down the document |
| `--select X1,Y1,X2,Y2` | Drag a text selection between two coordinates |
| `--type TEXT` | Send characters and braced key names to the focused control |
| `--wait MS` | Allow scheduled work before simulated input |
| `--settle MS` | Allow scheduled work after simulated input |

These are snapshot controls. `--width`/`--height` also set the initial engine
viewport, but native window attributes and host behavior determine window size.
Use positive sizes and valid arguments; the CLI has limited argument validation.
The PNG's physical size is viewport size multiplied by scale.

Only one pointer action is stored: supplying multiple `--hover`/`--press`/`--click`/
`--wheel` options keeps the last. Argument order does not define an action sequence.
Execution order is: load scripts, wait, pointer action, selection drag, typing,
settle, paint. For longer interaction sequences, drive the DOM from a dedicated
test script or use an embedding host.

## Examples

```sh
# Hover, then let its transition progress.
cargo run -p nile-shell -- examples/animations.html --png /tmp/nile-hover.png --hover 60,50 --settle 150

# Focus an input and type a name, then remove its last character.
cargo run -p nile-shell -- examples/forms.html --png /tmp/nile-form.png --click 60,66 --type 'Ada{Backspace}'

# Run timers before a snapshot.
cargo run -p nile-shell -- examples/scripting.html --png /tmp/nile-timers.png --wait 400

# Render the desktop showcase at a chosen size.
cargo run -p nile-shell -- examples/workbench/inventory.html --png /tmp/nile-inventory.png --width 1280 --height 900 --scale 2
```

`--type` supports key names such as `{Enter}`, `{Backspace}`, `{ArrowLeft}`,
`{Home}`, `{End}`, `{Space}`, `{Shift+End}`, `{Ctrl+a}` and `{Cmd+c}`. Text input
goes to the focused control, so focus it with a click or script first. Braced text
is interpreted as a key token, rather than literal braces.

Snapshot runs use a private in-memory clipboard. They do not read or change the
system clipboard. Headless window commands are queued without a native window.

## Validate behavior and visuals

Terminal output includes `console.log` lines and script errors. A PNG can still
be written after a script error, so check the logs or a test completion marker.
The [workbench checks](../../examples/workbench/smoke.js) provide a working pattern:

```sh
cargo run -p nile-shell -- examples/workbench/smoke.html --png /tmp/nile-smoke.png --width 1280 --height 900
```

Look for `WORKBENCH CHECKS PASSED: 38`, and inspect snapshots after visual changes.

For engine development, use `cargo test --workspace` and
`cargo clippy --workspace --all-targets -- -D warnings`. For phase timings:

```sh
NILE_TRACE=1 cargo run -p nile-shell -- examples/stress.html --png /tmp/nile-stress.png
```
