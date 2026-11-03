"""Python callables exposed to page scripts as `host.<name>(...)`: JSON arguments in, JSON result out."""

import ctypes as C
import json
import sys
import traceback

from ._lib import HostFn


class Callback:
    """One registered function. Keeps the C callback and its last result alive, as the library copies the result at once."""

    def __init__(self, name, fn):
        self.name, self.fn, self._result = name, fn, None
        self.c_fn = HostFn(self._call)

    def _call(self, args_json, _user):
        try:
            args = json.loads(args_json.decode("utf-8")) if args_json else []
            value = self.fn(*args)
        except Exception:
            print(f"nile: host.{self.name} raised:\n{traceback.format_exc()}", file=sys.stderr)
            return None
        if value is None:
            return None
        self._result = C.create_string_buffer(json.dumps(value).encode("utf-8"))
        return C.addressof(self._result)
