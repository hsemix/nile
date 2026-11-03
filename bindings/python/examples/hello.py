"""A window whose page calls into Python.  Run:  python examples/hello.py   (headless: python examples/hello.py --png out.png)"""

import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))  # run from a checkout without installing
import nile  # noqa: E402

@nile.function
def greet(name):
    return f"Hello, {name}, from Python {sys.version_info.major}.{sys.version_info.minor}"

if __name__ == "__main__":
    page = os.path.join(os.path.dirname(os.path.abspath(__file__)), "hello.html")
    sys.exit(nile.main([page, *sys.argv[1:]]))
