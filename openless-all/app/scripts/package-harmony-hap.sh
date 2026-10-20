#!/usr/bin/env bash
#
# Build, sign and stage the HarmonyOS HAP for a release.
#
#   scripts/package-harmony-hap.sh
#
# Requires a Huawei DevEco / HarmonyOS SDK (`OPENLESS_HARMONY_TOOLCHAIN`, or
# ~/.local/share/harmony/current) plus release signing material
# (`OPENLESS_HARMONY_SIGNING_DIR`, see scripts/sign-harmony-hap.py). Both are
# available on a self-hosted runner or a developer machine, never on a hosted
# GitHub runner: the SDK download is account-gated.
#
# Output: target/harmony-packages/
#   OpenLess_harmony_arm64.hap   versioned artifact
#   OpenLess_harmony.hap         stable alias used by the website links
#   SHA256SUMS
set -euo pipefail

APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLCHAIN="${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}"
# Channels, matching how the artifacts are distributed:
#   release  production build for HUAWEI AppGallery (signed with release.*)
#   test     beta build for the AppGallery test channel (signed with test.*)
#   none     no store signing; the debug material is used when it is present
# The GitHub Release only ever carries the *unsigned* HAP.
CHANNEL="${OPENLESS_HARMONY_CHANNEL:-release}"
case "$CHANNEL" in
  release | test) SIGN_PREFIX="$CHANNEL" ;;
  none) SIGN_PREFIX="" ;;
  *) echo 'OPENLESS_HARMONY_CHANNEL must be release, test or none' >&2; exit 1 ;;
esac
SIGNING_DIR="${OPENLESS_HARMONY_SIGNING_DIR:-$HOME/.local/share/harmony/signing/release}"
CACHE_ROOT="${OPENLESS_CACHE_ROOT:-$APP_ROOT/target/harmony-cache}"
OUTPUT="${OPENLESS_HARMONY_OUTPUT:-$APP_ROOT/target/harmony-packages}"

# The command-line tools ship their own `hvigorw` and node; put them on PATH up
# front so the version stamp, `ohpm` and `hvigorw` are all found without relying
# on the caller's environment.
if [ -x "$TOOLCHAIN/bin/hvigorw" ]; then
  PATH="$TOOLCHAIN/bin:$TOOLCHAIN/tool/node/bin:$PATH"
  export PATH
fi

[ -d "$TOOLCHAIN/sdk" ] || {
  echo "missing HarmonyOS SDK at $TOOLCHAIN (set OPENLESS_HARMONY_TOOLCHAIN)" >&2
  exit 1
}
if [ -n "$SIGN_PREFIX" ] && [ ! -d "$SIGNING_DIR" ]; then
  echo "missing signing directory $SIGNING_DIR (set OPENLESS_HARMONY_SIGNING_DIR)" >&2
  exit 1
fi

APP_JSON="$APP_ROOT/harmony/AppScope/app.json5"
VERSION="${OPENLESS_HARMONY_VERSION:-$(node -p "require('$APP_ROOT/package.json').version" | sed 's/[-+].*//')}"
VERSION_CODE="${OPENLESS_HARMONY_VERSION_CODE:-$(< "$APP_ROOT/linux-egui/package-revision")}"
[ -n "$VERSION" ] || { echo "cannot resolve the HarmonyOS versionName" >&2; exit 1; }
case "$VERSION_CODE" in
  '' | *[!0-9]*)
    echo "versionCode must be a positive integer, got '$VERSION_CODE'" >&2
    exit 1
    ;;
esac

# AppGallery rejects a versionCode that does not increase, and the manifest in
# the repository is a development placeholder (0.1.0 / 1). Patch the checkout for
# the duration of the build and restore it afterwards, including on failure.
BACKUP="$(mktemp)"
cp "$APP_JSON" "$BACKUP"
restore_app_json() { cp "$BACKUP" "$APP_JSON"; rm -f "$BACKUP"; }
trap restore_app_json EXIT
python3 - "$APP_JSON" "$VERSION" "$VERSION_CODE" <<'PY'
import re, sys
path, version, code = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
text = re.sub(r'"versionCode":\s*\d+', '"versionCode": ' + code, text)
text = re.sub(r'"versionName":\s*"[^"]*"', '"versionName": "' + version + '"', text)
open(path, 'w').write(text)
PY

# build-harmony-frontend.sh builds the Rust cdylib, the ArkWeb bundle, runs
# hvigor and (when debug material is present) signs. We sign again below with the
# release material so the artifact matches what AppGallery accepts.
# On a hosted runner there is no DevEco install, so `hvigorw` does not exist:
# scripts/prepare-harmony-sdk.sh installs hvigor next to the SDK instead.
HVIGOR_BIN=""
if [ -f "$TOOLCHAIN/standalone-hvigor/node_modules/@ohos/hvigor/bin/hvigor.js" ]; then
  HVIGOR_BIN="$TOOLCHAIN/standalone-hvigor/node_modules/@ohos/hvigor/bin/hvigor.js"
fi
if [ -z "$HVIGOR_BIN" ] && ! command -v hvigorw > /dev/null 2>&1; then
  echo "no hvigor available: install the SDK with scripts/prepare-harmony-sdk.sh or use a machine with DevEco Studio" >&2
  exit 1
fi

HVIGOR_JS="$HVIGOR_BIN" \
OPENLESS_CACHE_ROOT="$CACHE_ROOT" \
OPENLESS_HARMONY_TOOLCHAIN="$TOOLCHAIN" \
OPENLESS_HARMONY_SIGNING_DIR="$SIGNING_DIR" \
  bash "$APP_ROOT/scripts/build-harmony-frontend.sh"

BUILD_PROJECT="$CACHE_ROOT/harmony/openless-project"
OUT_DIR="$BUILD_PROJECT/entry/build/default/outputs/default"
UNSIGNED=$(ls "$OUT_DIR"/*-unsigned.hap 2>/dev/null | head -1)
[ -n "$UNSIGNED" ] || { echo "hvigor produced no unsigned HAP in $OUT_DIR" >&2; exit 1; }

SIGNED=""
if [ -n "$SIGN_PREFIX" ]; then
  python3 "$APP_ROOT/scripts/sign-harmony-hap.py" \
    "$TOOLCHAIN" "$SIGNING_DIR" "$OUT_DIR" --prefix "$SIGN_PREFIX"
  SIGNED="$OUT_DIR/entry-default-signed.hap"
  [ -f "$SIGNED" ] || { echo "expected a signed HAP at $SIGNED" >&2; exit 1; }
else
  echo "no store signing: packaging the unsigned HAP only"
fi

rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"
# The repository and the GitHub Release distribute the unsigned package, so a
# download never carries someone else's signing identity.
cp "$UNSIGNED" "$OUTPUT/OpenLess_harmony_arm64.hap"
cp "$UNSIGNED" "$OUTPUT/OpenLess_harmony.hap"
if [ -n "$SIGNED" ]; then
  # Store-only artifact: the workflow uploads it to AppGallery and keeps it out
  # of the Release.
  cp "$SIGNED" "$OUTPUT/OpenLess_harmony_arm64-signed.hap"
fi
( cd "$OUTPUT" && sha256sum ./*.hap > SHA256SUMS )
echo "HAP packages ready in $OUTPUT"
ls -l "$OUTPUT"
