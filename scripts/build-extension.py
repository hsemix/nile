#!/usr/bin/env python3
"""Build and assemble an ordinary-filesystem native extension package."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "aarch64-apple-darwin": ("macos-aarch64", "lib", ".dylib"),
    "x86_64-apple-darwin": ("macos-x86_64", "lib", ".dylib"),
    "x86_64-unknown-linux-gnu": ("linux-x86_64-gnu", "lib", ".so"),
    "aarch64-unknown-linux-gnu": ("linux-aarch64-gnu", "lib", ".so"),
    "x86_64-unknown-linux-musl": ("linux-x86_64-musl", "lib", ".so"),
    "aarch64-unknown-linux-musl": ("linux-aarch64-musl", "lib", ".so"),
    "x86_64-pc-windows-msvc": ("windows-x86_64-msvc", "", ".dll"),
    "aarch64-pc-windows-msvc": ("windows-aarch64-msvc", "", ".dll"),
}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name", choices=("sqlite", "notifications"))
    parser.add_argument("--dest", type=Path, required=True, help="output package directory")
    parser.add_argument("--target", help="Rust target triple (must match the Nile application)")
    parser.add_argument("--release", action="store_true")
    args = parser.parse_args()
    host = next(line[6:] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    target = args.target or host
    if target not in TARGETS:
        parser.error(f"unsupported native package target: {target}")
    platform, prefix, suffix = TARGETS[target]
    command = ["cargo", "build", "-p", f"nile-ext-{args.name}", "--target-dir", str(ROOT / "target")]
    if args.target:
        command += ["--target", target]
    if args.release:
        command += ["--release"]
    subprocess.run(command, cwd=ROOT, check=True)
    binary_dir = ROOT / "target"
    if args.target:
        binary_dir /= target
    binary_dir /= "release" if args.release else "debug"
    binary_name = f"{prefix}nile_ext_{args.name}{suffix}"
    native = Path("native") / platform / binary_name
    (args.dest / native).parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary_dir / binary_name, args.dest / native)
    shutil.copy2(ROOT / "extensions" / args.name / "package/index.js", args.dest / "index.js")
    # Preserve already assembled targets, allowing a distributor to combine platform builds.
    manifest_path = args.dest / "extension.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {
        "id": args.name, "version": "0.1.0", "abi": 1, "entry": "index.js", "native": {}
    }
    manifest["native"][platform] = native.as_posix()
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Assembled {args.dest} ({platform})")

if __name__ == "__main__":
    main()
