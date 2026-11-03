"""Find and load libnile_app, and declare its C functions (nile.h, nile_app.h)."""

import ctypes as C
import os
import sys
from pathlib import Path

ABI_VERSION = 2

_NAMES = {"darwin": "libnile_app.dylib", "win32": "nile_app.dll"}.get(sys.platform, "libnile_app.so")

HostFn = C.CFUNCTYPE(C.c_void_p, C.c_char_p, C.c_void_p)


class Tick(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("redraw", C.c_bool), ("next_ms", C.c_int32)]


class WindowCommand(C.Structure):
    _fields_ = [("struct_size", C.c_uint32), ("kind", C.c_int32), ("a", C.c_float), ("b", C.c_float)]


def _candidates():
    env = os.environ.get("NILE_LIBRARY")
    if env:
        yield Path(env)
    here = Path(__file__).resolve().parent
    yield here / "_native" / _NAMES
    for root in [here.parent, *here.parents]:
        for profile in ("release", "debug"):
            yield root / "target" / profile / _NAMES


def _load():
    tried = []
    for path in _candidates():
        if path.is_file():
            if sys.platform == "win32":
                os.add_dll_directory(str(path.parent))
            lib = C.CDLL(str(path))
            _declare(lib)
            if lib.nile_abi_version() != ABI_VERSION:
                raise RuntimeError(f"{path} is Nile ABI {lib.nile_abi_version()}, these bindings are for {ABI_VERSION}")
            return lib
        tried.append(str(path))
    raise ImportError(f"{_NAMES} not found; build it with `cargo build -p nile-app --release` or set NILE_LIBRARY. Looked in: " + ", ".join(tried[:4]))


def _declare(lib):
    p, s, f, b, u8 = C.c_void_p, C.c_char_p, C.c_float, C.c_bool, C.c_uint8
    sig = {
        "nile_abi_version": (C.c_uint32, []),
        "nile_version": (s, []),
        "nile_view_create_html": (p, [s, s, f, f]),
        "nile_view_create_file": (p, [s, f, f]),
        "nile_view_destroy": (None, [p]),
        "nile_view_reload": (None, [p]),
        "nile_string_free": (None, [p]),
        "nile_view_resize": (None, [p, f, f]),
        "nile_view_content_height": (f, [p]),
        "nile_view_render": (b, [p, C.c_void_p, C.c_size_t, C.c_uint32, C.c_uint32, f]),
        "nile_pointer_move": (b, [p, f, f]),
        "nile_pointer_leave": (b, [p]),
        "nile_pointer_button": (b, [p, C.c_int32, b, u8]),
        "nile_wheel": (b, [p, f, f]),
        "nile_key": (b, [p, s, u8, b]),
        "nile_text_input": (b, [p, s]),
        "nile_view_tick": (None, [p, C.POINTER(Tick)]),
        "nile_view_selected_text": (p, [p]),
        "nile_view_title": (s, [p]),
        "nile_view_eval": (C.c_int32, [p, s, C.POINTER(p)]),
        "nile_view_call": (C.c_int32, [p, s, s, C.POINTER(p)]),
        "nile_view_register_function": (b, [p, s, HostFn, p]),
        "nile_view_take_console": (p, [p]),
        "nile_view_write_pdf": (C.c_int32, [p, s]),
        "nile_view_set_storage_file": (C.c_int32, [p, s]),
        "nile_app_register_function": (b, [s, HostFn, p]),
        "nile_run": (C.c_int32, [C.c_int32, C.POINTER(s)]),
        "nile_run_file": (C.c_int32, [s]),
        "nile_run_html": (C.c_int32, [s, s, f, f]),
    }
    for name, (res, args) in sig.items():
        fn = getattr(lib, name)
        fn.restype, fn.argtypes = res, args


_lib = None


def lib():
    global _lib
    if _lib is None:
        _lib = _load()
    return _lib


def take_string(ptr):
    """Copy a library-owned `char *` into a str and free it."""
    if not ptr:
        return ""
    text = C.string_at(ptr).decode("utf-8", "replace")
    lib().nile_string_free(ptr)
    return text
