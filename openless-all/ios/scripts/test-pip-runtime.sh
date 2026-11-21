#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$(mktemp -d "${TMPDIR:-/tmp}/openless-pip-runtime.XXXXXX")"
trap 'rm -rf "$BUILD"' EXIT
xcrun clang -fobjc-arc -c "$ROOT/Tests/PiP/coexistence_fixture.m" -o "$BUILD/fixture.o"
xcrun swiftc -swift-version 6 -default-isolation MainActor -import-objc-header "$ROOT/Tests/PiP/coexistence_fixture.h" \
  "$ROOT/App/PiPCoexistenceAdapter.swift" "$ROOT/App/PiPLockRecovery.swift" "$ROOT/Tests/PiP/coexistence_runtime.swift" \
  "$BUILD/fixture.o" -o "$BUILD/pip-runtime"
"$BUILD/pip-runtime"
