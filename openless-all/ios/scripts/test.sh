#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SUITE="${1:-unit}"
case "$SUITE" in
  unit|ui|all) ;;
  *) echo 'Usage: scripts/test.sh unit|ui|all' >&2; exit 2;;
esac
if [ "$SUITE" = unit ] || [ "$SUITE" = all ]; then
  python3 "$ROOT/scripts/prepare-core.py"
  export RUSTC="$(rustup which --toolchain 1.99.0 rustc)"
  export RUSTDOC="$(rustup which --toolchain 1.99.0 rustdoc)"
  swift test --package-path "$ROOT/Packages/OpenLessShared"
  rustup run 1.99.0 cargo test --manifest-path "$ROOT/CoreHost/Cargo.toml" --workspace --lib --locked
  "$ROOT/scripts/test-rime.sh"
  bash "$ROOT/scripts/test-predictions.sh"
  "$ROOT/scripts/test-audio.sh"
fi
if [ "$SUITE" = ui ] || [ "$SUITE" = all ]; then
  : "${IOS_SIMULATOR_ID:?Set IOS_SIMULATOR_ID to a dedicated iOS QA simulator}"
  mkdir -p "$ROOT/Artifacts"
  RESULT="${UI_RESULT_BUNDLE:-$ROOT/Artifacts/ui-$(date +%Y%m%d-%H%M%S)-$$.xcresult}"
  # XCTest needs a second launcher while running. Keep it out of the product
  # preview afterwards, without uninstalling OpenLess or deleting its data.
  cleanup_ui_runner() {
    local info="$ROOT/DerivedData/Build/Products/Personal-iphonesimulator/OpenLess.app/Info.plist"
    if [ -f "$info" ]; then
      local bundle
      bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info")
      xcrun simctl uninstall "$IOS_SIMULATOR_ID" "$bundle.uitests.xctrunner" >/dev/null 2>&1 || true
    fi
  }
  trap cleanup_ui_runner EXIT
  GENERATOR="$(bash "$ROOT/scripts/resolve-xcodegen.sh")"
  (cd "$ROOT"; "$GENERATOR" generate)
  xcodebuild -project "$ROOT/OpenLess.xcodeproj" -scheme OpenLess -configuration Personal \
    -destination "platform=iOS Simulator,id=$IOS_SIMULATOR_ID" \
    -resultBundlePath "$RESULT" -collect-test-diagnostics never \
    -parallel-testing-enabled NO -maximum-concurrent-test-simulator-destinations 1 \
    -derivedDataPath "$ROOT/DerivedData" CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= test
  python3 "$ROOT/scripts/validate-product.py" \
    "$(xcrun simctl get_app_container "$IOS_SIMULATOR_ID" \
      "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$ROOT/DerivedData/Build/Products/Personal-iphonesimulator/OpenLess.app/Info.plist")" app)"
fi
