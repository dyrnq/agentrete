#!/usr/bin/env python3
"""Download sqlite-vec loadable extensions for all platforms.

Usage:
    python3 scripts/download-sqlite-vec.py              # download v0.1.9 to ext/
    python3 scripts/download-sqlite-vec.py 0.1.10       # specific version
    python3 scripts/download-sqlite-vec.py --latest     # auto-detect latest
    python3 scripts/download-sqlite-vec.py -d /tmp/ext  # custom output dir
"""

import argparse
import json
import os
import shutil
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

REPO = "asg017/sqlite-vec"
DEFAULT_VERSION = "0.1.9"
PLATFORMS = {
    "linux-x86_64":   ("linux", "x86_64", "vec0-linux-x86_64.so"),
    "linux-aarch64":  ("linux", "aarch64", "vec0-linux-aarch64.so"),
    "macos-x86_64":   ("macos", "x86_64", "vec0-macos-x86_64.dylib"),
    "macos-aarch64":  ("macos", "aarch64", "vec0-macos-aarch64.dylib"),
    "windows-x86_64": ("windows", "x86_64", "vec0-windows-x86_64.dll"),
}

def get_latest_version():
    url = f"https://api.github.com/repos/{REPO}/releases/latest"
    req = urllib.request.Request(url, headers={"User-Agent": "agentrete"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read())
    return data["tag_name"].lstrip("v")

def download_platform(version, platform_key, out_dir):
    platform = PLATFORMS[platform_key]
    tag = f"v{version}"
    base = f"sqlite-vec-{version}-loadable-{platform[0]}-{platform[1]}"
    url = f"https://github.com/{REPO}/releases/download/{tag}/{base}.tar.gz"
    out_name = platform[2]

    print(f"  {platform_key:18s} → {out_name}")
    try:
        with tempfile.TemporaryDirectory() as tmp:
            tgz = os.path.join(tmp, f"{base}.tar.gz")
            urllib.request.urlretrieve(url, tgz)
            extract_dir = os.path.join(tmp, "extract")
            os.makedirs(extract_dir, exist_ok=True)
            with tarfile.open(tgz) as tar:
                tar.extractall(extract_dir)
            # Find the vec0 binary and copy it
            for root, _, files in os.walk(extract_dir):
                for f in files:
                    if f in ("vec0", "vec0.dll", "vec0.dylib", "vec0.so"):
                        src = os.path.join(root, f)
                        dst = os.path.join(out_dir, out_name)
                        shutil.copy2(src, dst)
                        return dst
        print(f"    WARNING: binary not found in archive")
    except Exception as e:
        print(f"    FAILED: {e}")
    return None

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("version", nargs="?", default=DEFAULT_VERSION, help=f"Version (default: {DEFAULT_VERSION})")
    parser.add_argument("--latest", action="store_true", help="Auto-detect latest version")
    parser.add_argument("-d", "--dir", default="ext", help="Output directory (default: ext/)")
    parser.add_argument("-p", "--platform", choices=list(PLATFORMS.keys()), help="Download single platform only")
    args = parser.parse_args()

    if args.latest:
        print(f"Fetching latest version from {REPO}...")
        version = get_latest_version()
        print(f"  latest: {version}")
    else:
        version = args.version

    out_dir = Path(args.dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"sqlite-vec v{version} → {out_dir}/")

    platforms = [args.platform] if args.platform else PLATFORMS.keys()
    ok = 0
    for p in platforms:
        result = download_platform(version, p, out_dir)
        if result:
            size = os.path.getsize(result)
            print(f"    {size:>10,} bytes")
            ok += 1

    print(f"\nDone: {ok}/{len(platforms)} platforms → {out_dir.absolute()}/")

if __name__ == "__main__":
    main()
