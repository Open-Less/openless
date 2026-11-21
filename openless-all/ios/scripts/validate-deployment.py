#!/usr/bin/env python3
"""Reject products/static objects built above the promised iOS deployment floor."""
import argparse
import plistlib
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("files", nargs="*", type=Path)
parser.add_argument("--app", type=Path)
parser.add_argument("--max-ios", default="16.0")
args = parser.parse_args()


def version(value):
    return tuple((list(map(int, value.split("."))) + [0, 0, 0])[:3])


maximum = version(args.max_ios)
files = {path: maximum for path in args.files}


def add_native(path, ceiling):
    # A dependency inspected in several contexts must satisfy the lowest floor.
    files[path] = min(files.get(path, ceiling), ceiling)


def add_frameworks(directory, ceiling):
    for path in sorted(directory.glob("*.dylib")):
        add_native(path, ceiling)
    for framework in sorted(directory.glob("*.framework")):
        with (framework / "Info.plist").open("rb") as stream:
            metadata = plistlib.load(stream)
        if metadata.get("MinimumOSVersion") and version(metadata["MinimumOSVersion"]) > ceiling:
            parser.error(f"Unsupported framework MinimumOSVersion in {framework}: {metadata['MinimumOSVersion']}")
        add_native(framework / metadata["CFBundleExecutable"], ceiling)


if args.app:
    for bundle in [args.app, *sorted((args.app / "PlugIns").glob("*.appex"))]:
        with (bundle / "Info.plist").open("rb") as stream:
            info = plistlib.load(stream)
        minimum = info.get("MinimumOSVersion")
        if bundle == args.app:
            app_id = info["CFBundleIdentifier"]
        widget = (bundle != args.app
                  and info.get("CFBundleIdentifier") == app_id + ".recording"
                  and info.get("NSExtension", {}).get("NSExtensionPointIdentifier") == "com.apple.widgetkit-extension")
        ceiling = version("18.0") if widget else maximum
        if minimum is None or version(minimum) > ceiling:
            parser.error(f"Unsupported MinimumOSVersion in {bundle}: {minimum}")
        add_native(bundle / info["CFBundleExecutable"], ceiling)
        for path in sorted(bundle.glob("*.dylib")):
            add_native(path, ceiling)
        add_frameworks(bundle / "Frameworks", ceiling)
    # App-level shared frameworks were checked with the app ceiling above.
    # Only binaries private to the widget receive the iOS 18 ceiling.

if not files:
    parser.error("Provide native files or --app")

for path, ceiling in files.items():
    if not path.is_file():
        parser.error(f"Native product missing: {path}")
    process = subprocess.Popen(["xcrun", "otool", "-l", str(path)], stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True)
    versions = []
    invalid = []
    member = str(path)
    legacy = False
    for line in process.stdout:
        if line.rstrip().endswith("):"):
            member = line.strip()
        if "cmd LC_VERSION_MIN_IPHONEOS" in line:
            legacy = True
        match = re.match(r"\s*minos\s+(\d+(?:\.\d+){0,2})", line)
        if legacy:
            match = re.match(r"\s*version\s+(\d+(?:\.\d+){0,2})", line) or match
        if match:
            minimum = match.group(1)
            versions.append(minimum)
            legacy = False
            if version(minimum) > ceiling:
                invalid.append(f"{member}: iOS {minimum}")
    if process.wait() != 0 or not versions:
        parser.error(f"Could not inspect Mach-O deployment metadata: {path}")
    if invalid:
        parser.error("Deployment floor exceeded:\n" + "\n".join(invalid[:8]))
    print(f"Deployment verified: {path.name}; {len(versions)} Mach-O objects, max iOS {max(versions, key=version)}")
