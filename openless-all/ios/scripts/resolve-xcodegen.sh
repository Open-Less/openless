#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ -n "${OPENLESS_XCODEGEN_BIN:-}" ]; then
  [ -x "$OPENLESS_XCODEGEN_BIN" ] || { echo 'OPENLESS_XCODEGEN_BIN must name an executable.' >&2; exit 1; }
  printf '%s\n' "$OPENLESS_XCODEGEN_BIN"
elif [ -x "$ROOT/.build-tools/XcodeGen/.build/release/xcodegen" ]; then
  printf '%s\n' "$ROOT/.build-tools/XcodeGen/.build/release/xcodegen"
elif command -v xcodegen >/dev/null 2>&1; then
  command -v xcodegen
else
  echo 'XcodeGen is unavailable. Run scripts/bootstrap.sh or set OPENLESS_XCODEGEN_BIN to an installed executable.' >&2
  exit 1
fi
