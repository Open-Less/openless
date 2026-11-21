#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-simulator}"
CONFIG="${2:-Debug}"
SIGNING="${3:-unsigned}"
case "$PLATFORM" in
  simulator) SDK=iphonesimulator; DESTINATION='generic/platform=iOS Simulator';;
  device) SDK=iphoneos; DESTINATION='generic/platform=iOS';;
  *) echo 'Usage: scripts/build.sh simulator|device Debug|Personal|Release [unsigned|signed]' >&2; exit 2;;
esac
for library in libOpenLessCore.a libOpenLessRime.a; do
  [ -f "$ROOT/Frameworks/$PLATFORM/$library" ] || { echo "Missing $library: run scripts/build-core.sh and scripts/build-rime.sh." >&2; exit 1; }
done
python3 "$ROOT/scripts/validate-deployment.py" "$ROOT/Frameworks/$PLATFORM/libOpenLessCore.a" "$ROOT/Frameworks/$PLATFORM/libOpenLessRime.a"
[ -f "$ROOT/Resources/Rime/build/pinyin_simp.table.bin" ] || { echo 'Build the Rime data with scripts/build-rime.sh host.' >&2; exit 1; }
[ -f "$ROOT/Resources/Prediction/lexicon.sqlite3" ] || { echo 'Build offline predictions with scripts/build-predictions.py.' >&2; exit 1; }
GENERATOR="$(bash "$ROOT/scripts/resolve-xcodegen.sh")"
(cd "$ROOT"; "$GENERATOR" generate)
SIGN=(CODE_SIGNING_ALLOWED=NO)
VALIDATE_UNSIGNED=0
if [ "$SIGNING" = signed ]; then
  [ "$PLATFORM" = device ] || { echo 'Signing is only needed for the device build.' >&2; exit 2; }
  [ -f "$ROOT/Config/Local.xcconfig" ] || { echo 'Copy Config/Local.xcconfig.example and configure your own team and App Group.' >&2; exit 1; }
  if grep -Eq 'YOUR_TEAM_ID|yourname' "$ROOT/Config/Local.xcconfig"; then
    echo 'Replace the example identifiers before requesting provisioning.' >&2; exit 1
  fi
  SIGN=(-allowProvisioningUpdates)
elif [ "$SIGNING" != unsigned ]; then
  echo 'Expected unsigned or signed.' >&2; exit 2
elif [ "$PLATFORM" = simulator ]; then
  # Simulator App Groups still need Xcode's embedded simulated entitlements.
  SIGN=(CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM=)
else
  VALIDATE_UNSIGNED=1
fi
DERIVED="${OPENLESS_DERIVED_DATA:-$ROOT/DerivedData}"
xcodebuild -project "$ROOT/OpenLess.xcodeproj" -scheme OpenLess -configuration "$CONFIG" \
  -sdk "$SDK" -destination "$DESTINATION" -derivedDataPath "$DERIVED" -jobs "${OPENLESS_BUILD_JOBS:-4}" \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES "${SIGN[@]}" build
if [ "$VALIDATE_UNSIGNED" = 1 ]; then
  python3 "$ROOT/scripts/validate-product.py" --unsigned-device "$DERIVED/Build/Products/$CONFIG-$SDK/OpenLess.app"
else
  python3 "$ROOT/scripts/validate-product.py" "$DERIVED/Build/Products/$CONFIG-$SDK/OpenLess.app"
fi
echo "Product: $DERIVED/Build/Products/$CONFIG-$SDK/OpenLess.app"
if [ "$PLATFORM" = device ] && [ "$SIGNING" = unsigned ]; then echo 'Unsigned build: NOT installable on an iPhone.'; fi
