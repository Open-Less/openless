#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$ROOT/.build/tests"
xcrun swiftc -O -swift-version 6 -default-isolation MainActor \
  "$ROOT/Keyboard/KeyboardPredictionEngine.swift" \
  "$ROOT/scripts/prediction-tests.swift" -o "$ROOT/.build/tests/prediction-tests"
"$ROOT/.build/tests/prediction-tests" "$ROOT/Resources/Prediction/lexicon.sqlite3"
