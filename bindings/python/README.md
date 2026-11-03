# Nile for Python

Python bindings for [Nile](../../README.md), over `libnile_app` with `ctypes`: no compiler, no third-party packages.

```python
import nile

@nile.function                 # page scripts call host.add(1, 2)
def add(a, b):
    return a + b

nile.run("index.html")         # a window; returns when the last one closes
```

Or drive a page yourself, headless:

```python
with nile.View("<h1>Hi</h1>", width=300, height=100) as v:
    v.register("add", lambda a, b: a + b)
    v.eval("host.add(1, 2)")        # 3
    v.click(20, 20); v.type("text")
    v.save_png("out.png")
```

`nile.main([...])` runs the `nile` program itself (`--png`, `--pack`, ...); `python -m nile page.html` does the same.

## Setting up

```sh
cargo build -p nile-app --release          # builds libnile_app
python3 -m unittest discover -s bindings/python/tests
python3 bindings/python/examples/hello.py
```

The library is looked for in `NILE_LIBRARY`, then `nile/_native/`, then `target/release` and `target/debug` of the checkout.
Calling `nile.run` and friends must happen on the main thread (macOS requires it). Host functions run on that thread too, with
JSON arguments and a JSON-able return value; an exception is printed to stderr and the script gets `undefined`.

Not published to PyPI yet (wheels per platform come with the installers work).
