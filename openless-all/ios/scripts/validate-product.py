#!/usr/bin/env python3
"""Validate the built/installed app, not just its source asset catalog."""
import argparse
import hashlib
import json
import plistlib
import re
import subprocess
import sys
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("app", type=Path)
parser.add_argument("--unsigned-device", action="store_true",
                    help="Inspect an actually unsigned iPhoneOS build; does not verify signed entitlements or installability")
args = parser.parse_args()
app = args.app.resolve()


def require_file(path):
    if not path.is_file() or path.stat().st_size == 0:
        parser.error(f"Missing or empty product file: {path}")


def bundle_info(bundle, assets=True):
    require_file(bundle / "Info.plist")
    with (bundle / "Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    require_file(bundle / info["CFBundleExecutable"])
    if assets:
        require_file(bundle / "Assets.car")
    return info


def app_groups(bundle, metadata):
    """Use the signed entitlement, or the simulator's actual Mach-O section."""
    if args.unsigned_device:
        if metadata.get("CFBundleSupportedPlatforms") != ["iPhoneOS"]:
            parser.error("--unsigned-device only accepts iPhoneOS bundles")
        signature = subprocess.run(["codesign", "-d", str(bundle)], capture_output=True, text=True)
        if signature.returncode == 0 or "code object is not signed at all" not in signature.stderr:
            parser.error(f"--unsigned-device requires an actually unsigned bundle: {bundle}")
        declared = metadata.get("OLAppGroupIdentifier")
        if not declared or "$" in declared:
            parser.error(f"Missing resolved App Group declaration: {bundle}")
        return [declared]
    signed = subprocess.run(["codesign", "-d", "--entitlements", ":-", str(bundle)],
                            capture_output=True)
    if signed.returncode != 0:
        parser.error(f"Cannot read signed entitlements: {bundle}. For an unsigned iPhoneOS structure check, explicitly use --unsigned-device; it does not verify installation readiness.")
    try:
        entitlements = plistlib.loads(signed.stdout)
    except plistlib.InvalidFileException:
        parser.error(f"Could not read product entitlements: {bundle}")
    groups = entitlements.get("com.apple.security.application-groups")
    if not groups and "iPhoneSimulator" in metadata.get("CFBundleSupportedPlatforms", []):
        # Simulator ad-hoc signatures can be empty; Xcode embeds the effective
        # simulated entitlements in __TEXT,__entitlements instead. Never use a
        # source .entitlements file as a substitute for product evidence.
        result = subprocess.run(["xcrun", "otool", "-X", "-s", "__TEXT", "__entitlements",
                                 str(bundle / metadata["CFBundleExecutable"])],
                                capture_output=True, text=True, check=True)
        data = bytearray()
        for line in result.stdout.splitlines():
            match = re.fullmatch(r"[0-9a-fA-F]+\s+((?:[0-9a-fA-F]{2,8}\s*)+)", line)
            if match:
                for word in match.group(1).split():
                    data.extend(bytes.fromhex(word)[::-1])
        start = data.find(b"<?xml")
        end = data.find(b"</plist>", start)
        if start < 0 or end < 0:
            parser.error(f"Missing simulated product App Group entitlement: {bundle}")
        entitlements = plistlib.loads(data[start:end + len(b"</plist>")])
        groups = entitlements.get("com.apple.security.application-groups")
    expected = metadata.get("OLAppGroupIdentifier")
    candidates = [expected]
    bundle_id = metadata.get("CFBundleIdentifier", "")
    point = metadata.get("NSExtension", {}).get("NSExtensionPointIdentifier")
    suffixes = {"com.apple.keyboard-service": ".keyboard", "com.apple.widgetkit-extension": ".recording"}
    if point is None and bundle_id:
        candidates.append("group." + bundle_id)
    elif point in suffixes and bundle_id.endswith(suffixes[point]):
        candidates.append("group." + bundle_id[:-len(suffixes[point])])
    if not expected or "$" in expected or groups not in [[value] for value in candidates]:
        parser.error(f"Product has no matching declared or re-signed App Group entitlement: {bundle}")
    return groups


info = bundle_info(app)
for key, pattern in [("CFBundleShortVersionString", r"\d+\.\d+\.\d+"),
                     ("CFBundleVersion", r"[1-9]\d*")]:
    if not re.fullmatch(pattern, str(info.get(key, ""))):
        parser.error(f"Missing or unresolved app version: {key}")
primary = info.get("CFBundleIcons", {}).get("CFBundlePrimaryIcon", {})
if primary.get("CFBundleIconName") != "AppIcon":
    parser.error("The built Info.plist does not declare the AppIcon primary icon")
icon_files = primary.get("CFBundleIconFiles", [])
if not icon_files:
    parser.error("The built Info.plist has no SpringBoard icon files")
for name in icon_files:
    if not any(app.glob(f"{Path(name).stem}*.png")):
        parser.error(f"The compiled SpringBoard icon is missing: {name}")

extensions = list((app / "PlugIns").glob("*.appex"))
if len(extensions) != 2:
    parser.error(f"Expected exactly one keyboard and one recording widget, found {len(extensions)} extensions")
identified = {}
expected_extensions = {
    "com.apple.keyboard-service": ".keyboard",
    "com.apple.widgetkit-extension": ".recording",
}
for extension in extensions:
    metadata = bundle_info(extension, assets=False)
    for key in ["CFBundleShortVersionString", "CFBundleVersion"]:
        if metadata.get(key) != info[key]:
            parser.error(f"Extension version differs from the app: {extension}: {key}")
    point = metadata.get("NSExtension", {}).get("NSExtensionPointIdentifier")
    if point not in expected_extensions or point in identified:
        parser.error(f"Unexpected or duplicate extension point in {extension}: {point}")
    if metadata.get("CFBundleIdentifier") != info["CFBundleIdentifier"] + expected_extensions[point]:
        parser.error(f"Unexpected extension bundle identifier: {extension}")
    identified[point] = (extension, metadata)
keyboard, keyboard_info = identified["com.apple.keyboard-service"]
widget, widget_info = identified["com.apple.widgetkit-extension"]
require_file(keyboard / "Assets.car")
if widget_info.get("MinimumOSVersion") != "18.0":
    parser.error("The recording control widget must have an iOS 18.0 deployment floor")
if "NSMicrophoneUsageDescription" in widget_info or "NSSpeechRecognitionUsageDescription" in widget_info:
    parser.error("The recording widget must not own microphone or speech permissions")
groups = app_groups(app, info)
for extension, metadata in [(keyboard, keyboard_info), (widget, widget_info)]:
    if app_groups(extension, metadata) != groups:
        parser.error(f"Embedded extension App Group differs from the app: {extension}")
for path in widget.rglob("*"):
    if path.name == "SenseVoice" or path.suffix == ".onnx" or "openlesscore" in path.name.lower():
        parser.error(f"Speech model or Core library must not be bundled in the recording widget: {path}")
widget_natives = [widget / widget_info["CFBundleExecutable"], *widget.rglob("*.dylib")]
for framework in widget.rglob("*.framework"):
    metadata = bundle_info(framework, assets=False)
    widget_natives.append(framework / metadata["CFBundleExecutable"])
for native in widget_natives:
    symbols = subprocess.run(["xcrun", "nm", "-g", str(native)], capture_output=True,
                             text=True, check=True).stdout
    links = subprocess.run(["xcrun", "otool", "-L", str(native)], capture_output=True,
                           text=True, check=True).stdout
    if re.search(r"\b_?ol_ios_|openless_core|OpenLessCore", symbols + links):
        parser.error(f"The recording widget must not link the native speech Core: {native}")
require_file(keyboard / "Rime/build/pinyin_simp.table.bin")
require_file(keyboard / "Prediction/manifest.json")
require_file(keyboard / "Prediction/lexicon.sqlite3")
prediction_manifest = json.loads((keyboard / "Prediction/manifest.json").read_text())
if hashlib.sha256((keyboard / "Prediction/lexicon.sqlite3").read_bytes()).hexdigest() != prediction_manifest["database_sha256"]:
    parser.error("Packaged prediction dictionary does not match its source manifest")
# Verify actual product resources, not only the bootstrap download cache.
for name, expected in {
    "model.int8.onnx": "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
    "tokens.txt": "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
}.items():
    path = app / "SenseVoice" / name
    require_file(path)
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    if digest.hexdigest() != expected:
        parser.error(f"Bundled SenseVoice checksum mismatch: {path}")
if (keyboard / "SenseVoice").exists():
    parser.error("The speech model must not be bundled inside the keyboard")
subprocess.run([sys.executable, str(Path(__file__).with_name("validate-deployment.py")), "--app", str(app)], check=True)
if args.unsigned_device:
    print(f"Unsigned device structure verified: {info['CFBundleIdentifier']}; app/keyboard iOS 16, widget iOS 18, declared App Group IDs, icon, Rime and pinned speech resources. Signed entitlements, provisioning and iPhone installation are NOT verified.")
else:
    print(f"Product verified: {info['CFBundleIdentifier']}; app/keyboard iOS 16, recording widget iOS 18, matching product App Groups, icon, Rime and pinned local speech resources.")
