#!/usr/bin/env bash
# Shared, provider-free build/enumeration/mock gates. No live execution branch.
set -euo pipefail
umask 077
log=$(mktemp "${TMPDIR:-/tmp}/openless-intent-gates.XXXXXX")
trap 'rm -f "$log"' EXIT
fail() { printf 'FAIL intent_gate=%s\n' "$1" >&2; exit 1; }
[[ $# -ge 1 ]] || fail arguments

case "$1" in
  python)
    [[ $# == 1 ]] || fail arguments
    PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_intent_style_comparison_ci.py \
      > "$log" 2>&1 || fail python_dummy
    grep -Eq '^Ran 22 tests in ' "$log" || fail python_count
    grep -Fx 'OK' "$log" > /dev/null || fail python_result
    printf 'PASS python_tests=22\n'
    ;;
  build)
    [[ $# == 2 ]] || fail arguments
    # Exactly one compilation; stdout is only select-binary's safe assignment.
    cargo test --locked -p openless-core --lib -j 1 --no-run --message-format=json \
      > "$2" 2> "$log" || fail core_test_lib_compile
    python3 scripts/intent-style-comparison-ci.py select-binary "$2" \
      > "$log" 2>&1 || fail core_test_binary_selection
    cat "$log"
    printf 'PASS core_test_lib_compilations=1\n' >&2
    ;;
  verify)
    [[ $# == 2 ]] || fail arguments
    binary=$2
    [[ -f "$binary" && -x "$binary" ]] || fail test_binary
    listed_once() {
      local target=$1
      shift
      "$binary" "$target" "$@" --exact --list > "$log" 2>&1 || fail enumeration
      [[ "$(grep -Fxc "$target: test" "$log")" == 1 ]] || fail exact_name
      [[ "$(grep -c ': test$' "$log")" == 1 ]] || fail enumeration_count
    }
    for name in \
      intent_comparison_http_loop_32_and_36 \
      intent_comparison_http_loop_stops_on_first_error \
      intent_comparison_transport_budget_and_redirects \
      intent_comparison_transport_connect_request_and_timeout_stop \
      intent_comparison_legacy_limits_remain_independent; do
      listed_once "intent_style_tests::$name"
    done
    "$binary" intent_style_tests::intent_comparison_ --test-threads=1 \
      > "$log" 2>&1 || fail intent_mocks
    grep -Eq '^test result: ok\. 5 passed; 0 failed; 0 ignored;' "$log" || fail mock_count
    listed_once intent_style_tests::intent_live_configured_channel_comparison --ignored
    printf 'PASS mock_tests=5 exact_mock_names=5 ignored_live_listed=1 live_tests_executed=0\n'
    ;;
  *) fail operation ;;
esac
