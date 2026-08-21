#!/usr/bin/env bash
# Copy the dsh-jsonl plugin into OpenLess.
#
# The plugin's source of truth is github.com/bigsongeth/dsh-jsonl. OpenLess
# embeds the source with its provenance and MIT license (via include_str!) so
# the app needs no npm install and no network at runtime. Nothing syncs the two automatically —
# run this after the upstream plugin changes, then run the dsh live acceptance
# (see coding_agent::dsh::live) to confirm the Rust parser still agrees with it.
#
#   scripts/vendor-dsh-jsonl.sh ~/Personal/dsh-jsonl
set -euo pipefail

SRC="${1:-}"
if [[ -z "$SRC" || ! -f "$SRC/index.js" || ! -f "$SRC/package.json" || ! -f "$SRC/LICENSE" ]]; then
  echo "usage: $0 <path-to-dsh-jsonl-checkout>" >&2
  exit 2
fi

VERSION=$(node -p "require('$SRC/package.json').version")
DEST="$(cd "$(dirname "$0")/.." && pwd)/app/src-tauri/src/coding_agent/vendor/dsh-jsonl.js"

{
  echo "// Vendored from github.com/bigsongeth/dsh-jsonl v${VERSION} — do not edit here."
  echo "// Run scripts/vendor-dsh-jsonl.sh <path-to-checkout> to update, then bump"
  echo "// VENDORED_DSH_JSONL_VERSION in dsh.rs to match."
  echo "//"
  awk '{ print (length($0) ? "// " $0 : "//") }' "$SRC/LICENSE"
  echo "//"
  cat "$SRC/index.js"
} > "$DEST"

echo "vendored dsh-jsonl v${VERSION} -> ${DEST}"
echo "next: set VENDORED_DSH_JSONL_VERSION = \"${VERSION}\" in coding_agent/dsh.rs"
