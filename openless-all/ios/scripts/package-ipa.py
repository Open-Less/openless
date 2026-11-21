#!/usr/bin/env python3
"""Validate an unsigned device product and package it without overwriting an IPA."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import stat
import subprocess
import sys
import tempfile
import zipfile


def digest(stream):
    result = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        result.update(block)
    return result.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path, help="Release-iphoneos/OpenLess.app")
    parser.add_argument("output", type=Path, help="Destination .ipa; must not exist")
    args = parser.parse_args()
    app, output = args.app.resolve(), args.output.absolute()
    if app.suffix != ".app" or not app.is_dir():
        parser.error("Expected an existing .app directory")
    if output.suffix != ".ipa" or output.exists() or output.is_symlink():
        parser.error("Destination must be a new .ipa file")
    if app in output.resolve().parents:
        parser.error("Destination must be outside the app bundle")
    subprocess.run([sys.executable, str(Path(__file__).with_name("validate-product.py")),
                    "--unsigned-device", str(app)], check=True)
    with (app / "Info.plist").open("rb") as stream:
        metadata = plistlib.load(stream)
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".openless-ipa-", dir=output.parent)
    os.close(descriptor)
    temporary = Path(temporary)
    try:
        expected = {}
        with zipfile.ZipFile(temporary, "w", compression=zipfile.ZIP_DEFLATED,
                             compresslevel=6) as archive:
            for path in sorted(app.rglob("*")):
                if path.is_symlink():
                    raise ValueError(f"Unexpected symlink in device product: {path}")
                if not path.is_file() or path.name == ".DS_Store" or path.name.startswith("._"):
                    continue
                name = "Payload/" + str(path.relative_to(app.parent))
                with path.open("rb") as stream:
                    expected[name] = (digest(stream), stat.S_IMODE(path.stat().st_mode))
                archive.write(path, name)
        with zipfile.ZipFile(temporary) as archive:
            if set(archive.namelist()) != expected.keys() or archive.testzip() is not None:
                raise ValueError("Archive members or CRC differ from the device product")
            for name, (checksum, mode) in expected.items():
                with archive.open(name) as stream:
                    if digest(stream) != checksum:
                        raise ValueError(f"Archive content mismatch: {name}")
                if stat.S_IMODE(archive.getinfo(name).external_attr >> 16) != mode:
                    raise ValueError(f"Archive permissions mismatch: {name}")
        # A hard link publishes atomically and fails if another process created output.
        temporary.chmod(0o644)
        os.link(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)
    with output.open("rb") as stream:
        checksum = digest(stream)
    print(json.dumps({"ipa": str(output), "version": metadata["CFBundleShortVersionString"],
                      "build": metadata["CFBundleVersion"], "bytes": output.stat().st_size,
                      "sha256": checksum, "files": len(expected)}, indent=2))


if __name__ == "__main__":
    main()
