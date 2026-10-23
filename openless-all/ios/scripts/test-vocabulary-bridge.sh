#!/bin/bash
# Host-only verification. No simulator, microphone, user data, credentials or network.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/Sources/VocabularyBridgeHarness" "$WORK/Tests/VocabularyBridgeHarnessTests"
cp "$ROOT/App/VocabularyBridge.swift" "$WORK/Sources/VocabularyBridgeHarness/"
cp "$ROOT/AppTests/VocabularyBridgeTests.swift" "$WORK/Tests/VocabularyBridgeHarnessTests/"
cat > "$WORK/Package.swift" <<EOF
// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "VocabularyBridgeHarness", platforms: [.macOS(.v14)],
 dependencies: [.package(path: "$ROOT/Packages/OpenLessShared")], targets: [
 .target(name: "VocabularyBridgeHarness", dependencies: [.product(name: "OpenLessShared", package: "OpenLessShared")]),
 .testTarget(name: "VocabularyBridgeHarnessTests", dependencies: ["VocabularyBridgeHarness", .product(name: "OpenLessShared", package: "OpenLessShared")])])
EOF
swift test --package-path "$WORK"
# A temporary standalone Rust library includes only the owned module. This
# exercises real Core repositories without editing the parent's lib/control seams.
mkdir -p "$WORK/rust/src"
cat > "$WORK/rust/Cargo.toml" <<EOF
[package]
name = "ios-vocabulary-bridge-harness"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
openless-core = { path = "$ROOT/../app/crates/openless-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
uuid = { version = "1", features = ["v4"] }
[dev-dependencies]
tempfile = "3"
EOF
cat > "$WORK/rust/src/lib.rs" <<EOF
#[path = "$ROOT/CoreHost/src/vocabulary_bridge.rs"]
mod vocabulary_bridge;
EOF
CARGO_TARGET_DIR="$ROOT/CoreHost/target" cargo test --offline --manifest-path "$WORK/rust/Cargo.toml" --lib
