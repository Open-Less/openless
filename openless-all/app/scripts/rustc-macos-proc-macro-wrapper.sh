#!/usr/bin/env bash

# Tauri sets MACOSX_DEPLOYMENT_TARGET for the application target. On Apple
# Silicon, Rust proc-macro dylibs built with that target cannot be loaded by
# rustc on this toolchain, even though regular application crates can use it.
# Keep the target for normal crates and use the host default only for proc
# macros.

set -euo pipefail

RUSTC="$1"
shift

is_proc_macro=0
previous_arg=""
for arg in "$@"; do
  if [[ "$arg" == "--crate-type=proc-macro" || ( "$previous_arg" == "--crate-type" && "$arg" == "proc-macro" ) ]]; then
    is_proc_macro=1
    break
  fi
  previous_arg="$arg"
done

if (( is_proc_macro )); then
  unset MACOSX_DEPLOYMENT_TARGET
fi

exec "$RUSTC" "$@"
