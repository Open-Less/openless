#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$ROOT/.build/rime-host/install"
mkdir -p "$ROOT/.build/tests"
xcrun clang++ -std=c++17 -fobjc-arc -framework Foundation -I"$ROOT/.vendor/librime/src" \
  "$ROOT/scripts/rime-smoke.mm" "$ROOT/RimeBridge/OLRimeSession.mm" \
  -L"$LIB/lib" -lrime "-Wl,-rpath,$LIB/lib" -o "$ROOT/.build/tests/rime-smoke"
"$ROOT/.build/tests/rime-smoke" "$ROOT/Resources/Rime"
