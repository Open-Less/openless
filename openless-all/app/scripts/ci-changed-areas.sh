#!/usr/bin/env bash
#
# Decide whether a heavy cross-platform check must run for this change set.
#
#   ci-changed-areas.sh <area> [base-sha]
#
# Areas: tauri | msrv | linux | harmony | frontend | android | capsule | ios-device
#
# The decision is made by scripts/ci-impact.mjs, which walks Cargo's own package
# graph (`cargo metadata`) from each platform's root package: a file is in scope
# for a platform only when the crate that owns it is reachable from that
# platform's root. Non-Rust trees (frontend bundle, ArkTS, Xcode project,
# tooling) are covered by explicit rules there.
#
# This wrapper exists for two reasons only: it keeps the shell entry point that
# ci.yml and the contract test use, and it re-asserts the fail-open rule on top
# of the script — if the decision cannot be computed at all (node missing, the
# script crashing, unparsable output), the answer is "run".
#
# Prints "true" when the check must run and "false" only when every changed file
# is provably out of scope for that area. Anything unknown - no base revision
# (push, tag, manual dispatch), a git failure, or an empty diff - prints "true".
# Skipping a platform check must always be the result of a change set that
# cannot reach it, never of a failure to read the change set.
set -uo pipefail

area=${1:?usage: ci-changed-areas.sh <tauri|msrv|linux|harmony|frontend|android|capsule|ios-device> [base-sha]}
base=${2:-}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
script="$repo_root/openless-all/app/scripts/ci-impact.mjs"

run() {
  echo true
  exit 0
}

command -v node > /dev/null 2>&1 || run
[ -f "$script" ] || run

verdict=$(node "$script" "$area" "$base" 2> /dev/null) || run

case "$verdict" in
  true | false) echo "$verdict" ;;
  *) run ;;
esac
