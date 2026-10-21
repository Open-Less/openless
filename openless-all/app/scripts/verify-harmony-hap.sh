#!/usr/bin/env bash
#
# Verify a packaged HarmonyOS HAP or App Pack without a device.
#
#   scripts/verify-harmony-hap.sh <hap|app> [toolchain] [versionName] [versionCode] [--allow-unsigned]
#
# Checks the things that silently go wrong in a HAP build: a missing ArkWeb
# bundle, a stale version stamp, a missing native library, and - when the Huawei
# signing tool is available - an invalid signature. An App Pack (`.app`, what
# AppGallery accepts) is unwrapped first and checked the same way.
set -euo pipefail

hap=${1:?usage: verify-harmony-hap.sh <hap> [toolchain] [versionName] [versionCode]}
toolchain=${2:-${OPENLESS_HARMONY_TOOLCHAIN:-$HOME/.local/share/harmony/current}}
version_name=${3:-}
version_code=${4:-}

[ -f "$hap" ] || { echo "missing HAP: $hap" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# An App Pack wraps one HAP; check the inner package, and check the pack's own
# signature when the signing tool is available.
if [ "${hap##*.}" = "app" ]; then
  inner=$(unzip -Z1 "$hap" | grep -E '\.hap$' | head -1)
  [ -n "$inner" ] || { echo "App Pack contains no HAP" >&2; exit 1; }
  unzip -q -o "$hap" "$inner" -d "$work"
  echo "App Pack: $hap (inner $inner)"
  # The pack carries the signature; sign-app was run on the `.app`, not on the
  # HAP it wraps, so the signature check below targets the shipped file.
  signature_target=$hap
  hap="$work/$inner"
fi

entries=$(unzip -Z1 "$hap")
require_entry() {
  grep -qx "$1" <<< "$entries" || { echo "HAP is missing $1" >&2; exit 1; }
}
require_entry module.json
require_entry 'libs/arm64-v8a/libopenless_harmony.so'
require_entry 'resources/rawfile/web/index.html'
# The ArkWeb bundle is hashed on every build: a fixed name would hide a stale copy.
grep -q '^resources/rawfile/web/assets/index-.*\.js$' <<< "$entries" \
  || { echo "HAP has no hashed ArkWeb entry bundle" >&2; exit 1; }

python3 - "$hap" "$version_name" "$version_code" <<'PY'
import json
import subprocess
import sys

hap, want_name, want_code = sys.argv[1:4]
module = json.loads(subprocess.run(['unzip', '-p', hap, 'module.json'],
                                   capture_output=True, check=True).stdout)
app = module['app']
if want_name and str(app.get('versionName')) != want_name:
    sys.exit(f"versionName is {app.get('versionName')}, expected {want_name}")
if want_code and str(app.get('versionCode')) != want_code:
    sys.exit(f"versionCode is {app.get('versionCode')}, expected {want_code}")
module_name = module['module']['name']
print(f"HAP: {module_name} versionName={app.get('versionName')} versionCode={app.get('versionCode')}")
PY

jar=$toolchain/sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar
if [ "${5:-}" = '--allow-unsigned' ]; then
  # The GitHub Release carries the unsigned package on purpose.
  echo "signature check skipped (unsigned artifact)"
elif [ -f "$jar" ] && command -v java > /dev/null 2>&1; then
  # verify-app exits non-zero on a tampered or unsigned package; keep its output
  # terse because a valid run prints several informational lines.
  if out=$(java -jar "$jar" verify-app -inFile "${signature_target:-$hap}" -outCertChain "$hap.cer" \
      -outProfile "$hap.p7b" 2>&1); then
    echo "signature verified"
  else
    echo "$out" >&2
    echo "HAP signature verification failed" >&2
    exit 1
  fi
  rm -f "$hap.cer" "$hap.p7b"
else
  echo "skipping signature verification: hap-sign-tool.jar or java not available"
fi

echo "verified $hap"
