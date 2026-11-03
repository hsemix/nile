"""A page you drive yourself: load it, call into it, send input, and read pixels back (headless or into your own window)."""

import ctypes as C
import json

from . import _png
from ._host import Callback
from ._lib import Tick, lib, take_string

MOD_SHIFT, MOD_CONTROL, MOD_ALT, MOD_META = 1, 2, 4, 8


class ScriptError(Exception):
    """A script error raised in the page."""


class View:
    def __init__(self, source, *, width=800, height=600, base_dir=None, file=False):
        """`source` is HTML text, or with `file=True` the path of an HTML file."""
        self.width, self.height = width, height
        L = lib()
        if file:
            self._p = L.nile_view_create_file(str(source).encode(), float(width), float(height))
        else:
            self._p = L.nile_view_create_html(source.encode(), str(base_dir).encode() if base_dir else None, float(width), float(height))
        if not self._p:
            raise ValueError("could not create the view")
        self._callbacks = {}

    @classmethod
    def from_file(cls, path, **kw):
        return cls(path, file=True, **kw)

    def close(self):
        if self._p:
            lib().nile_view_destroy(self._p)
            self._p = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def __del__(self):
        try:
            self.close()
        except Exception:
            pass

    def _live(self):
        if not self._p:
            raise ValueError("the view is closed")
        return self._p

    @property
    def title(self):
        return (lib().nile_view_title(self._live()) or b"").decode()

    def resize(self, width, height):
        self.width, self.height = width, height
        lib().nile_view_resize(self._live(), float(width), float(height))

    def reload(self):
        lib().nile_view_reload(self._live())

    @property
    def content_height(self):
        return lib().nile_view_content_height(self._live())

    # --- scripts ---------------------------------------------------------------------------------------------------
    def register(self, name, fn=None):
        """Expose `fn` to the page as `host.<name>(...)`; usable as a decorator: `@view.register("add")`."""
        if fn is None:
            return lambda f: self.register(name, f)
        cb = Callback(name, fn)
        self._callbacks[name] = cb
        lib().nile_view_register_function(self._live(), name.encode(), cb.c_fn, None)
        return fn

    def eval(self, code):
        """Run JavaScript; strings come back as `str`, anything else decoded from JSON, `undefined` as `None`."""
        out = C.c_void_p()
        status = lib().nile_view_eval(self._live(), code.encode(), C.byref(out))
        text = take_string(out.value)
        if status:
            raise ScriptError(text)
        if not text:
            return None
        try:
            return json.loads(text)
        except ValueError:
            return text

    def call(self, name, *args):
        """Call a page function (`"save"` or dotted `"app.save"`) with JSON-able arguments."""
        out = C.c_void_p()
        status = lib().nile_view_call(self._live(), name.encode(), json.dumps(list(args)).encode(), C.byref(out))
        text = take_string(out.value)
        if status:
            raise ScriptError(text)
        return json.loads(text) if text else None

    def console(self):
        """The `console.log/warn/error` lines since the last call."""
        text = take_string(lib().nile_view_take_console(self._live()))
        return text.split("\n") if text else []

    # --- input -----------------------------------------------------------------------------------------------------
    def pointer_move(self, x, y):
        return lib().nile_pointer_move(self._live(), float(x), float(y))

    def pointer_leave(self):
        return lib().nile_pointer_leave(self._live())

    def pointer_button(self, button, pressed, modifiers=0):
        return lib().nile_pointer_button(self._live(), button, pressed, modifiers)

    def click(self, x, y, modifiers=0):
        self.pointer_move(x, y)
        self.pointer_button(0, True, modifiers)
        return self.pointer_button(0, False, modifiers)

    def wheel(self, dx, dy):
        return lib().nile_wheel(self._live(), float(dx), float(dy))

    def key(self, key, modifiers=0):
        """Press and release a DOM `KeyboardEvent.key` ("a", "Enter", "ArrowLeft", ...)."""
        lib().nile_key(self._live(), key.encode(), modifiers, True)
        return lib().nile_key(self._live(), key.encode(), modifiers, False)

    def type(self, text):
        return lib().nile_text_input(self._live(), text.encode())

    def selected_text(self):
        return take_string(lib().nile_view_selected_text(self._live()))

    def tick(self):
        """Run timers and animations. Returns `(redraw, next_ms)`; `next_ms` is None when nothing is scheduled."""
        t = Tick(struct_size=C.sizeof(Tick))
        lib().nile_view_tick(self._live(), C.byref(t))
        return t.redraw, (None if t.next_ms < 0 else t.next_ms)

    def run_for(self, ms):
        """Let the page's timers run for about `ms` milliseconds (for scripts that update later)."""
        import time

        end = time.monotonic() + ms / 1000
        while time.monotonic() < end:
            _, nxt = self.tick()
            time.sleep(min(nxt if nxt is not None else 10, 10) / 1000)

    # --- pixels ----------------------------------------------------------------------------------------------------
    def render(self, scale=1.0):
        """The page as premultiplied BGRA bytes, `width*scale` x `height*scale`."""
        w, h = round(self.width * scale), round(self.height * scale)
        buf = (C.c_uint8 * (w * h * 4))()
        if not lib().nile_view_render(self._live(), buf, len(buf), w, h, float(scale)):
            raise ValueError("render failed")
        return bytes(buf)

    def save_png(self, path, scale=1.0):
        w, h = round(self.width * scale), round(self.height * scale)
        with open(path, "wb") as f:
            f.write(_png.encode(bytearray(self.render(scale)), w, h))

    def write_pdf(self, path):
        if lib().nile_view_write_pdf(self._live(), str(path).encode()) != 0:
            raise OSError("could not write the PDF")
