#!/usr/bin/env bash
# Only the explicit Dev job calls this. No installation or release/update publishing.
set -euo pipefail
[[ "$CI" == true && "$(uname -m)" == arm64 ]]
cd "$(dirname "$0")/../../app"
evidence="$RUNNER_TEMP/openless-dev-evidence"
mkdir -p "$evidence"
export CARGO_BUILD_JOBS=1 APPLE_SIGNING_IDENTITY=-
source ../macos/scripts/macos-build-env.sh

# Compile just these small production-module harnesses, reusing one debug dependency tree.
check() {
  cargo test -j1 --manifest-path src-tauri/backend-tests/Cargo.toml --locked --test "$1" "$2" -- --test-threads=1 2>&1 | tee "$evidence/current-test.log" | tee -a "$evidence/targeted-tests.log"
  grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$evidence/current-test.log"
}
check skip_auto_style_restore skip_auto_style
check capsule_picker_command command_precheck_classifies_old_style_requests_before_geometry
check capsule_style_menu real_core_style_mutations_fence_pending_prepared_active_and_late_host_delivery
check selection_bound_native original_range_and_complete_text_must_both_match_before_submission
check clipboard_delivery selection_success_restores_complete_original_and_next_borrow_never_restores_result_a
check browser_capture_authorization unchanged_original_grant_flows_from_capture_through_bind_to_the_real_turn
check browser_capture_authorization revoke_regrant_inside_freeze_discards_successful_native_result_and_preserves_ordinary_capture
check browser_scene ''
check ask_native explicit_material_is_one_turn_user_data_and_close_revokes_a_prepared_send
check ask_native browser_reference_uses_the_actual_turn_and_original_invocation_once
check ask_native same_window_tab_away_and_back_cannot_revive_a_same_looking_reference
check ask_native ungranted_sources_and_later_turns_never_read_a_page

# Core's lib-test executable is compiled once and reused for these seven filters.
# Share the debug dependency directory with the small host harnesses above.
for filter in \
  history_feedback_confirmation_updates_next_recognition_vocabulary_only_once \
  history_feedback_rewrite_punctuation_cancel_permissions_and_source_revision \
  history_feedback_rejects_translation_selection_unknown_candidates_and_expiry \
  history_expression_opt_in_is_abstract_and_once_per_session_on_feedback_day \
  native_snapshot_positive_path_and_permanent_discontinuities \
  legacy_selection_history_keeps_the_full_original_separate_from_the_result \
  missing_grants_and_protected_controls_never_create_a_reference; do
  CARGO_TARGET_DIR="$PWD/src-tauri/backend-tests/target" cargo test -j1 -p openless-core --lib --locked "$filter" -- --test-threads=1 2>&1 | tee "$evidence/current-test.log" | tee -a "$evidence/targeted-tests.log"
  grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$evidence/current-test.log"
done
# Do not run a second frontend build or a debug Tauri host build.
npm run tauri -- build --ci --features dev-build --bundles dmg --config src-tauri/tauri.dev.conf.json -- --locked --timings
app=src-tauri/target/release/bundle/macos/'OpenLess Dev.app'
lipo "$app/Contents/MacOS/openless" -verify_arch arm64
codesign --verify --deep --strict "$app"
bash ../macos/scripts/check-macos-audio-usage-description.sh "$app/Contents/Info.plist"
codesign -d --entitlements :- "$app" 2>/dev/null | grep -q com.apple.security.device.audio-input
[[ ! -e "$app.tar.gz" && ! -e "$app.tar.gz.sig" ]]
dmgs=(src-tauri/target/release/bundle/dmg/*.dmg)
[[ ${#dmgs[@]} == 1 && -f "${dmgs[0]}" ]]
dmg="${dmgs[0]}"

mount="$RUNNER_TEMP/openless-dev-mounted"
extracted="$RUNNER_TEMP/openless-dev-extracted"
mkdir -p "$mount" "$extracted"
mounted=false
cleanup() {
  if [[ "$mounted" == true ]]; then hdiutil detach "$mount" >/dev/null || true; fi
  rm -rf "$extracted"
}
trap cleanup EXIT
hdiutil attach -readonly -nobrowse -mountpoint "$mount" "$dmg" >/dev/null
mounted=true
ditto "$mount/OpenLess Dev.app" "$extracted/OpenLess Dev.app"
hdiutil detach "$mount" >/dev/null
mounted=false
codesign --verify --deep --strict "$extracted/OpenLess Dev.app"
swift ../macos/scripts/dev-macos-smoke.swift "$extracted/OpenLess Dev.app" "$OPENLESS_DEV_VERSION" "$evidence"

# These values were created by this job only, before any app launch; never print them.
[[ "$(security find-generic-password -s com.openless.app -a credentials.v2 -w)" == openless-dev-ci-formal-sentinel ]]
[[ "$(security find-generic-password -s com.openless.app.siwc -a vault.v1 -w)" == openless-dev-ci-siwc-sentinel ]]
python3 ../macos/scripts/dev-macos-provenance.py verify "$dmg"
delivery="$RUNNER_TEMP/openless-dev-delivery"
mkdir -p "$delivery"
cp "$dmg" "$delivery/"
cp "$evidence/manifest.json" "$delivery/"
# The extracted bundle is removed by the EXIT trap, after the Swift smoke reaps its child.
