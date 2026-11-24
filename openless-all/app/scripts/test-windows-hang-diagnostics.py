#!/usr/bin/env python3
"""Actions-only harness importing the production sink, without Tauri/native DLLs.

The real Windows writer is compiled by the feature-on host check. Unit tests use
injected memory/failing writers, never the user's session file. The facade binary
also executes the non-test, disabled path (including feature-on macOS).
"""
import json
from pathlib import Path
import subprocess
import tempfile

source = Path(__file__).resolve().parents[1] / "src-tauri/src/issue1266_debug.rs"
with tempfile.TemporaryDirectory(prefix="openless-hang-tests-") as directory:
    root = Path(directory)
    (root / "src/bin").mkdir(parents=True)
    (root / "Cargo.toml").write_text('''[package]
name = "openless-hang-tests"
version = "0.1.0"
edition = "2021"
[features]
issue-1266-diagnostics = []
[dependencies]
parking_lot = "0.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
''', encoding="utf-8")
    module = "#[path = " + json.dumps(str(source), ensure_ascii=False) + "]\nmod issue1266_debug;\n"
    (root / "src/lib.rs").write_text(module, encoding="utf-8")
    (root / "src/bin/facade.rs").write_text(module + '''fn main() {
    if !issue1266_debug::enabled() {
        issue1266_debug::initialize();
        let health = serde_json::to_value(issue1266_debug::health()).unwrap();
        assert_eq!(health["initialized"], false);
        assert_eq!(health["ready"], false);
        let op = issue1266_debug::Operation::new(issue1266_debug::Probe::P1);
        op.mark(issue1266_debug::Event::Requested);
    }
}
''', encoding="utf-8")
    for features in ([], ["--features", "issue-1266-diagnostics"]):
        for command in (["test", "--lib"], ["run", "--bin", "facade"]):
            subprocess.run(["cargo", *command, "--manifest-path", str(root / "Cargo.toml"), *features], check=True)
