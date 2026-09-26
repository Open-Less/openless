#!/usr/bin/env bash
#
# Decide whether a heavy cross-platform check must run for this change set.
#
#   ci-changed-areas.sh <area> [base-sha]
#
# Areas:
#   tauri   macOS / Windows desktop Tauri builds and the Android build that
#           shares the same dependency graph.
#   msrv    The Rust 1.88 minimum-supported-toolchain compile.
#   linux   The Linux host build, its tests and the deb/rpm package chain.
#
# Prints "true" when the check must run and "false" only when every changed file
# is provably out of scope for that area. Anything unknown - no base revision
# (push, tag, manual dispatch), a git failure, or an empty diff - prints "true".
# Skipping a platform check must always be the result of a change set that
# cannot reach it, never of a failure to read the change set.
set -uo pipefail

area=${1:?usage: ci-changed-areas.sh <tauri|msrv> [base-sha]}
base=${2:-}

run() {
  echo true
  exit 0
}

case "$area" in
  tauri | msrv | linux) ;;
  *)
    echo "unknown area: $area" >&2
    run
    ;;
esac

# Push, tag and manual runs verify every platform.
[ -n "$base" ] || run

files=$(git diff --name-only "${base}...HEAD" 2> /dev/null) || run
[ -n "$files" ] || run

while IFS= read -r file; do
  [ -n "$file" ] || continue
  case "$area" in
    tauri)
      # Only the Linux host, its fcitx5 plugin and prose are out of scope for
      # the desktop Tauri build. The Linux host manifest stays in scope: it is
      # part of the shared workspace and moves Cargo.lock for every platform.
      case "$file" in
        openless-all/app/linux-egui/Cargo.toml) run ;;
        openless-all/app/linux-egui/*) ;;
        openless-all/scripts/linux-fcitx5-plugin/*) ;;
        docs/* | *.md | LICENSE* | NOTICE*) ;;
        *) run ;;
      esac
      ;;
    msrv)
      # The 1.88 promise is about Rust sources and their dependency graph;
      # only change sets that contain neither may skip that compile.
      case "$file" in
        *.rs | *Cargo.toml | *Cargo.lock | *rust-toolchain* | .github/*) run ;;
        *) ;;
      esac
      ;;
    linux)
      # Prose cannot change what the host builds, tests or packages.
      case "$file" in
        docs/* | *.md | LICENSE* | NOTICE*) ;;
        *) run ;;
      esac
      ;;
  esac
done <<< "$files"

echo false
