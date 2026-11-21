#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Read-only host library supplied by bootstrap; tests write only to mktemp dirs.
LIB="${RIME_HOST_INSTALL:-$ROOT/.build/rime-host/install}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
xcrun clang++ -std=c++17 -fobjc-arc -framework Foundation -I"$ROOT/.vendor/librime/src" \
  "$ROOT/scripts/rime-vocabulary-tests.mm" "$ROOT/RimeBridge/OLRimeSession.mm" \
  -L"$LIB/lib" -lrime "-Wl,-rpath,$LIB/lib" -o "$WORK/rime-vocabulary-tests"
"$WORK/rime-vocabulary-tests" "$ROOT/Resources/Rime"
