#!/usr/bin/env bash
# Validate a HarmonyOS signing directory before a build or a dispatch.
#
#   bash scripts/check-harmony-signing.sh <signing-dir> [release|test|debug]
#
# Checks the three files Huawei issues/downloads against each other:
#   * <prefix>.p12 opens with the stored password and holds an alias
#   * <prefix>.cer belongs to that alias (the public keys must match), otherwise
#     AppGallery accepts the certificate and the device rejects the HAP
#   * <prefix>.p7b names the same bundleName as harmony/AppScope/app.json5, was
#     issued for that certificate, has a type that matches the channel, and both
#     the profile and the certificate are inside their validity window
#
# Read-only: nothing is written outside a temporary directory and no secret is
# printed.
set -euo pipefail

APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
signing=${1:-}
prefix=${2:-release}
[ -n "$signing" ] || {
  echo "usage: $0 <signing-dir> [release|test|debug]" >&2
  exit 2
}
[ -d "$signing" ] || {
  echo "error: $signing is not a directory" >&2
  exit 2
}

failures=0
ok() { echo "  ok    $*"; }
bad() {
  echo "  FAIL  $*" >&2
  failures=$((failures + 1))
}
warn() { echo "  warn  $*"; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "signing directory: $signing (channel: $prefix)"

# 1. Files -------------------------------------------------------------------
for extension in p12 cer p7b; do
  file="$signing/$prefix.$extension"
  if [ -s "$file" ]; then ok "$prefix.$extension present"; else bad "missing or empty $file"; fi
done
[ "$failures" -eq 0 ] || {
  echo "cannot continue without the three files" >&2
  exit 1
}

# 2. Password ----------------------------------------------------------------
password=${HARMONY_KEYSTORE_PASSWORD:-}
if [ -z "$password" ] && [ -f "$signing/keystore-password" ]; then
  password=$(< "$signing/keystore-password")
fi
[ -n "$password" ] || {
  bad "no password: set HARMONY_KEYSTORE_PASSWORD or write $signing/keystore-password"
  exit 1
}

# 3. Keystore and aliases ----------------------------------------------------
if keytool -list -keystore "$signing/$prefix.p12" -storetype PKCS12 \
  -storepass "$password" > "$work/aliases.txt" 2> "$work/keytool.err"; then
  ok "keystore opens with the stored password"
else
  bad "keystore rejects the password ($(tail -1 "$work/keytool.err" | tr -d '\r'))"
  echo >&2
  echo "FAILED: cannot read the keystore" >&2
  exit 1
fi
aliases=$(sed -n 's/^\([^,]*\),.*/\1/p' "$work/aliases.txt" | sed 's/^[[:space:]]*//')
requested=${HARMONY_KEY_ALIAS:-harmony-$prefix}
if printf '%s\n' "$aliases" | grep -qx "$requested"; then
  ok "alias '$requested' is in the keystore"
  alias=$requested
elif [ "$(printf '%s\n' "$aliases" | wc -l)" -eq 1 ]; then
  alias=$(printf '%s\n' "$aliases" | head -1)
  warn "alias '$requested' not found, using the only alias present: '$alias'"
else
  alias=$(printf '%s\n' "$aliases" | head -1)
  bad "alias '$requested' not in the keystore (found: $(printf '%s' "$aliases" | tr '\n' ' '))"
fi

# 4. Certificate belongs to the key, profile belongs to the certificate ------
# `release.cer` / `debug.cer` from AppGallery Connect are PEM chains (Huawei root
# CA + leaf). The leaf is identified by its public key, because the first
# certificate in the file is the CA.
keytool -exportcert -rfc -keystore "$signing/$prefix.p12" -storetype PKCS12 \
  -storepass "$password" -alias "$alias" > "$work/keystore.pem" 2> /dev/null ||
  bad "cannot export the certificate of alias '$alias'"

if [ -s "$work/keystore.pem" ]; then
  # The helper reads a certificate chain and reports what it found; it never
  # exits non-zero, so the bash-level failure counter stays authoritative.
  python3 "$APP_ROOT/scripts/harmony-signing-report.py" \
    "$signing/$prefix.cer" "$work/keystore.pem" "$signing/$prefix.p7b" ||
    failures=$((failures + 1))
fi

# 5. Profile matches the application -----------------------------------------
if openssl cms -verify -noverify -inform DER -in "$signing/$prefix.p7b" \
  -out "$work/profile.json" 2> "$work/cms.err"; then
  ok "profile opens"
  python3 "$APP_ROOT/scripts/harmony-profile-report.py" \
    "$work/profile.json" "$APP_ROOT/harmony/AppScope/app.json5" "$prefix" ||
    failures=$((failures + 1))
else
  bad "cannot read $prefix.p7b ($(tail -1 "$work/cms.err" | tr -d '\r'))"
fi

echo
if [ "$failures" -gt 0 ]; then
  echo "FAILED: $failures problem(s) in $signing" >&2
  exit 1
fi
echo "PASS: $signing/$prefix.{p12,cer,p7b} are consistent and current"
