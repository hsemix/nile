"""Nile for Python: an HTML/CSS/JS UI engine with its own renderer (no webview).

    import nile

    @nile.function
    def add(a, b):
        return a + b

    nile.run("index.html")          # a window; scripts call host.add(1, 2)

or drive a page yourself, headless:

    with nile.View("<h1>Hi</h1>", width=300, height=100) as v:
        v.save_png("out.png")
"""

import ctypes as C
import sys

from ._host import Callback
from ._lib import lib
from .view import MOD_ALT, MOD_CONTROL, MOD_META, MOD_SHIFT, ScriptError, View

__all__ = ["View", "ScriptError", "function", "run", "run_html", "main", "version", "MOD_SHIFT", "MOD_CONTROL", "MOD_ALT", "MOD_META"]

_functions = {}


def version():
    return lib().nile_version().decode()


def function(arg=None, *, name=None):
    """Expose a Python function to every window as `host.<name>(...)`. Use bare (`@nile.function`) or with a name."""

    def register(fn):
        key = name or fn.__name__
        cb = Callback(key, fn)
        _functions[key] = cb  # keep it alive for the whole run
        if not lib().nile_app_register_function(key.encode(), cb.c_fn, None):
            raise ValueError(f"could not register {key!r}")
        return fn

    return register(arg) if callable(arg) else register


def run(path=None):
    """Open the HTML file (or folder) in a window and run until the last window closes. Returns the exit code."""
    return lib().nile_run_file((str(path) if path is not None else ".").encode())


def run_html(html, *, base_dir=None, width=0, height=0):
    """Show HTML text in a window."""
    return lib().nile_run_html(html.encode(), str(base_dir).encode() if base_dir else None, float(width), float(height))


def main(argv=None):
    """Run the `nile` program itself with command-line arguments (`page.html`, `--png out.png`, `--pack`, ...)."""
    args = [sys.argv[0] if argv is None else "nile"] + list(sys.argv[1:] if argv is None else argv)
    arr = (C.c_char_p * len(args))(*[a.encode() for a in args])
    return lib().nile_run(len(args), arr)
