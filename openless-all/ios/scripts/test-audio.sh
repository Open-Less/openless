#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$ROOT/.build/tests"
xcrun swiftc -swift-version 6 -target arm64-apple-macos27.0 -parse-as-library \
  "$ROOT/App/Speech/AudioPCM.swift" "$ROOT/scripts/ReferencePCMWorker.swift" "$ROOT/scripts/audio-smoke.swift" \
  -o "$ROOT/.build/tests/audio-smoke"
"$ROOT/.build/tests/audio-smoke"
