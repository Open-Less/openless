#!/usr/bin/env python3
"""Static delivery boundaries; not a substitute for iPhone runtime tests."""
import json
import plistlib
import re
import struct
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[1]
def plist(name):
    with (root / "Config" / name).open("rb") as stream:
        return plistlib.load(stream)

project = (root / "project.yml").read_text()
assert 'IPHONEOS_DEPLOYMENT_TARGET: "16.0"' in project, "Keep the app and keyboard deployment floor at iOS 16"
spec = json.loads(subprocess.check_output(
    ["ruby", "-ryaml", "-rjson", "-e", "puts JSON.generate(YAML.load_file(ARGV.fetch(0)))", str(root / "project.yml")],
    text=True))
targets = spec["targets"]
global_floor = spec["settings"]["base"]["IPHONEOS_DEPLOYMENT_TARGET"]
for name in ["OpenLess", "OpenLessKeyboard"]:
    target = targets[name]
    assert str(target.get("deploymentTarget", global_floor)) == "16.0", f"{name} must retain iOS 16"
    assert str(target.get("settings", {}).get("base", {}).get("IPHONEOS_DEPLOYMENT_TARGET", global_floor)) == "16.0"
widget_target = targets["OpenLessRecordingWidget"]
assert str(widget_target["deploymentTarget"]) == "18.0", "Only the recording widget requires iOS 18"
assert widget_target["dependencies"] == [{"package": "OpenLessShared"}], "Widget must not depend on LocalSpeech or native Core"
allowed_widget_sources = {
    "RecordingWidget", "PlatformShared/RecordingActivityIntent.swift",
    "PlatformShared/KeyboardHostIntent.swift", "PlatformShared/SharedContainer.swift",
    "Resources", "Resources/PrivacyInfo.xcprivacy",
}
for source in widget_target["sources"]:
    path = source if isinstance(source, str) else source["path"]
    assert path in allowed_widget_sources, f"Unexpected widget source/resource dependency: {path}"
    if path == "Resources":
        assert source.get("includes") == ["en.lproj/**", "zh-Hans.lproj/**"]
assert widget_target["settings"]["base"]["APPLICATION_EXTENSION_API_ONLY"] is True
for package in ["OpenLessShared", "LocalSpeech"]:
    assert ".iOS(.v16)" in (root / "Packages" / package / "Package.swift").read_text()
for path in (root / "App").rglob("*.swift"):
    assert "import Observation" not in path.read_text(), "Do not introduce an iOS 17-only observation runtime"
app = plist("App-Info.plist")
keyboard = plist("Keyboard-Info.plist")
widget = plist("RecordingWidget-Info.plist")
version = (root / "Config/Version.xcconfig").read_text()
assert re.search(r"^MARKETING_VERSION = \d+\.\d+\.\d+$", version, re.MULTILINE)
assert re.search(r"^CURRENT_PROJECT_VERSION = [1-9]\d*$", version, re.MULTILINE)
assert '#include "Version.xcconfig"' in (root / "Config/Base.xcconfig").read_text()
for metadata in [app, keyboard, widget]:
    assert metadata["CFBundleShortVersionString"] == "$(MARKETING_VERSION)"
    assert metadata["CFBundleVersion"] == "$(CURRENT_PROJECT_VERSION)"
assert app["NSMicrophoneUsageDescription"]
assert app["UIBackgroundModes"] == ["audio"]
extension = keyboard["NSExtension"]
assert extension["NSExtensionPointIdentifier"] == "com.apple.keyboard-service"
assert extension["NSExtensionAttributes"]["RequestsOpenAccess"] is True
assert "NSMicrophoneUsageDescription" not in keyboard
assert plist("App.entitlements")["com.apple.security.application-groups"] == plist("Keyboard.entitlements")["com.apple.security.application-groups"]
assert widget["NSExtension"]["NSExtensionPointIdentifier"] == "com.apple.widgetkit-extension"
assert "NSMicrophoneUsageDescription" not in widget and "NSSpeechRecognitionUsageDescription" not in widget
assert plist("App.entitlements")["com.apple.security.application-groups"] == plist("RecordingWidget.entitlements")["com.apple.security.application-groups"]
assert json.loads((root / "Resources/KeepAliveCompatibility.json").read_text()) == [], "Do not fabricate device evidence"
for path in (root / "Keyboard").glob("*.swift"):
    text = path.read_text()
    assert "AVAudioEngine" not in text and "SFSpeechRecognizer" not in text and "URLSession" not in text
# Widget strings must share the app and keyboard localization inventory.
inventory = json.loads((root / "Resources/LocalizationSources.json").read_text())["entries"]
token = re.compile(r'"(?:\\.|[^"\\])*"')
han = re.compile(r'[\u3400-\u9fff]')
locale_keys = []
for language in ["en", "zh-Hans"]:
    lines = (root / f"Resources/{language}.lproj/Localizable.strings").read_text().splitlines()
    locale_keys.append({json.loads(match.group(1)) for line in lines
                        if (match := re.fullmatch(r'("(?:\\.|[^"\\])*") = ("(?:\\.|[^"\\])*");', line))})
for path in sorted((root / "RecordingWidget").rglob("*.swift")):
    text = path.read_text()
    assert all(item not in text for item in ["AVAudioEngine", "SFSpeechRecognizer", "URLSession", "import LocalSpeech", "import OpenLessCore"])
    for number, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("//"):
            continue
        for literal in token.findall(line):
            if han.search(literal) and "\\(" not in literal:
                key = json.loads(literal)
                assert all(key in keys for keys in locale_keys), f"Missing widget localization: {path}:{number}: {key}"
                assert key in inventory, f"Missing widget localization provenance: {key}"
                assert any(ref.startswith(f"{path.relative_to(root)}:") for ref in inventory[key]["callsites"])
canonical = root.parent / "app/src-tauri/icons"
brand = root / "Resources/Brand.xcassets/OpenLessBrand.imageset/OpenLessBrand.png"
# The in-product mark must be transparent; SpringBoard uses a separate opaque tile.
subprocess.run(["swift", "-e", r"""
import Foundation
import ImageIO
import CoreGraphics
let path = CommandLine.arguments.last!
let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)!
let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
let context = CGContext(data: nil, width: image.width, height: image.height, bitsPerComponent: 8,
  bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
let bytes = context.data!.assumingMemoryBound(to: UInt8.self)
let count = image.width * image.height
let transparent = (0..<count).filter { bytes[$0 * 4 + 3] == 0 }.count
precondition(transparent > count / 2 && transparent < count, "Brand must retain its mark on actual transparency")
precondition(bytes[((image.height / 2) * image.width + image.width / 2) * 4 + 3] == 0, "The mark interior must be transparent")
""", str(brand)], check=True)
icon = (root / "Resources/Assets.xcassets/AppIcon.appiconset/AppIcon.png").read_bytes()
assert icon[:8] == b"\x89PNG\r\n\x1a\n"
width, height, depth, color_type, *_ = struct.unpack(">IIBBBBB", icon[16:29])
assert (width, height, depth, color_type) == (1024, 1024, 8, 2), "iOS app icon must be opaque 1024px RGB"
offset = 8
while offset < len(icon):
    length = struct.unpack(">I", icon[offset:offset + 4])[0]
    assert icon[offset + 4:offset + 8] != b"tRNS", "App icons cannot contain a transparency chunk"
    offset += length + 12
assert (root / "Resources/Rime/build/pinyin_simp.table.bin").is_file(), "Build licensed Rime assets first"
assert not (root / "Config/Local.xcconfig").exists() or "YOUR_TEAM_ID" not in (root / "Config/Local.xcconfig").read_text()
print("App/keyboard iOS 16, widget iOS 18, widget localization/no-speech dependencies, transparent brand, privacy, App Groups and unverified-PiP boundaries passed.")
