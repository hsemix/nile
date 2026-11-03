import os
import struct
import tempfile
import unittest

import nile


class ViewTests(unittest.TestCase):
    def test_version_and_title(self):
        self.assertTrue(nile.version())
        with nile.View("<title>Hello</title><p>hi</p>") as v:
            self.assertEqual(v.title, "Hello")

    def test_eval_and_call(self):
        html = "<script>function twice(n){return n*2} var app={name:'x'}</script>"
        with nile.View(html) as v:
            self.assertEqual(v.eval("1 + 2"), 3)
            self.assertEqual(v.eval("'text'"), "text")
            self.assertEqual(v.eval("undefined"), None)
            self.assertEqual(v.eval("({a: [1, 2]})"), {"a": [1, 2]})
            self.assertEqual(v.call("twice", 21), 42)
            with self.assertRaises(nile.ScriptError):
                v.eval("nope(")

    def test_host_functions_both_ways(self):
        seen = []
        with nile.View("<p>x</p>") as v:
            @v.register("add")
            def add(a, b):
                seen.append((a, b))
                return a + b

            v.register("boom", lambda: 1 / 0)
            self.assertEqual(v.eval("host.add(1, 2)"), 3)
            self.assertEqual(seen, [(1, 2)])
            self.assertEqual(v.eval("typeof host.boom()"), "undefined")

    def test_input_changes_the_page(self):
        html = "<button id=b onclick=\"document.title='clicked'\" style='width:100px;height:40px'>Go</button><input id=i style='display:block'>"
        with nile.View(html, width=300, height=200) as v:
            v.click(20, 20)
            self.assertEqual(v.title, "clicked")
            v.eval("document.getElementById('i').focus()")
            v.type("abc")
            self.assertEqual(v.eval("document.getElementById('i').value"), "abc")

    def test_render_and_png(self):
        with nile.View("<body style='margin:0;background:#f00'>", width=20, height=10) as v:
            px = v.render()
            self.assertEqual(len(px), 20 * 10 * 4)
            self.assertEqual(px[:4], bytes([0, 0, 255, 255]))  # BGRA
            path = os.path.join(tempfile.mkdtemp(), "o.png")
            v.save_png(path)
            with open(path, "rb") as f:
                data = f.read()
            self.assertEqual(data[:8], b"\x89PNG\r\n\x1a\n")
            self.assertEqual(struct.unpack(">II", data[16:24]), (20, 10))

    def test_closed_view_refuses(self):
        v = nile.View("<p>x</p>")
        v.close()
        with self.assertRaises(ValueError):
            v.eval("1")


if __name__ == "__main__":
    unittest.main()


class AppTests(unittest.TestCase):
    def test_main_renders_headless_and_calls_python(self):
        @nile.function
        def shout(s):
            return s.upper() + "!"

        d = tempfile.mkdtemp()
        page, png = os.path.join(d, "p.html"), os.path.join(d, "p.png")
        with open(page, "w") as f:
            f.write("<body style='margin:0;background:#00f'><script>document.title = host.shout('hey')</script>")
        self.assertEqual(nile.main([page, "--png", png, "--width", "30", "--height", "20"]), 0)
        with open(png, "rb") as f:
            self.assertEqual(f.read(8), b"\x89PNG\r\n\x1a\n")
