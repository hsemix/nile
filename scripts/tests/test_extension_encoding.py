"""Exercise extension scripts with UTF-8 tool output that is invalid cp1252."""
import io
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


class ExtensionEncodingTests(unittest.TestCase):
    def utf8_output(self, raw):
        def output(*args, **kwargs):
            self.assertEqual(kwargs.get("encoding"), "utf-8")
            self.assertEqual(kwargs.get("errors", "strict"), "strict")
            return raw.decode(kwargs.get("encoding", "cp1252"), errors=kwargs.get("errors", "strict"))
        return output

    def test_standalone_metadata_is_decoded_strictly_as_utf8(self):
        raw = json.dumps({"packages": [{"name": "native-名"}]}, ensure_ascii=False).encode("utf-8")
        with self.assertRaises(UnicodeDecodeError):
            raw.decode("cp1252")
        with patch.dict(os.environ), patch("subprocess.check_output", side_effect=self.utf8_output(raw)), \
                patch("subprocess.run") as build, patch("sys.stdout", new_callable=io.StringIO):
            runpy.run_path(str(ROOT / "scripts/check-extension-standalone.py"))
            build.assert_called_once()

    def test_package_builder_decodes_rustc_as_utf8(self):
        script = runpy.run_path(str(ROOT / "scripts/build-extension.py"))
        raw = "rustc version 名\nhost: aarch64-apple-darwin\n".encode("utf-8")
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(sys, "argv", ["build-extension.py", "sqlite", "--dest", directory]), \
                patch("subprocess.check_output", side_effect=self.utf8_output(raw)), \
                patch("subprocess.run"), patch("shutil.copy2"), \
                patch("sys.stdout", new_callable=io.StringIO):
            script["main"]()
            manifest = json.loads((Path(directory) / "extension.json").read_text(encoding="utf-8"))
            self.assertIn("macos-aarch64", manifest["native"])


if __name__ == "__main__":
    unittest.main()
