#!/usr/bin/env python3
"""Verify an extension builds outside the Nile workspace using only the public SDK."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

os.environ["CARGO_NET_OFFLINE"] = "true"
root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="nile-standalone-") as temporary:
    base = Path(temporary)
    shutil.copytree(root / "crates/nile-extension-sdk", base / "crates/nile-extension-sdk")
    shutil.copytree(root / "extensions/sqlite", base / "extensions/sqlite")
    manifest = str(base / "extensions/sqlite/Cargo.toml")
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--offline", "--format-version", "1", "--manifest-path", manifest
    ], text=True))
    names = {package["name"] for package in metadata["packages"]}
    forbidden = {"nile-script", "nile-engine", "nile-dom", "nile-style", "nile-layout", "nile-render", "nile-shell", "rquickjs"}
    assert not names & forbidden, f"SDK consumer acquired engine dependencies: {names & forbidden}"
    subprocess.run([
        "cargo", "build", "--offline", "--release", "--manifest-path", manifest,
        "--target-dir", str(root / "target/standalone-extensions")
    ], check=True)
    print("Standalone SQLite build passed; no engine or QuickJS dependencies.")
