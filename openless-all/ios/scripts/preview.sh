#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEVICE="${IOS_SIMULATOR_ID:?Set IOS_SIMULATOR_ID to the simulator used for preview}"
mkdir -p "$ROOT/Artifacts"
for library in libOpenLessCore.a libOpenLessRime.a; do
  [ -f "$ROOT/Frameworks/simulator/$library" ] || { echo "Missing $library; build native dependencies first." >&2; exit 1; }
done
[ -f "$ROOT/Resources/Rime/build/pinyin_simp.table.bin" ] || { echo 'Run scripts/build-rime.sh host first.' >&2; exit 1; }
python3 "$ROOT/scripts/validate-project.py"
GENERATOR="$(bash "$ROOT/scripts/resolve-xcodegen.sh")"
(cd "$ROOT"; "$GENERATOR" generate)
xcrun simctl bootstatus "$DEVICE" -b
# This is a product build, never a test run. No XCTest runner is installed.
xcodebuild -project "$ROOT/OpenLess.xcodeproj" -scheme OpenLess -configuration Personal \
  -destination "platform=iOS Simulator,id=$DEVICE" -derivedDataPath "$ROOT/DerivedData" \
  CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= build
APP="$ROOT/DerivedData/Build/Products/Personal-iphonesimulator/OpenLess.app"
python3 "$ROOT/scripts/validate-product.py" "$APP"
BUNDLE=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP/Info.plist")
xcrun simctl uninstall "$DEVICE" "$BUNDLE.uitests.xctrunner" >/dev/null 2>&1 || true
xcrun simctl install "$DEVICE" "$APP"
INSTALLED=$(xcrun simctl get_app_container "$DEVICE" "$BUNDLE" app)
python3 "$ROOT/scripts/validate-product.py" "$INSTALLED"
xcrun simctl launch --terminate-running-process "$DEVICE" "$BUNDLE"
printf 'Preview installed and launched: %s\n' "$BUNDLE"
