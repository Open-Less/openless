# OpenLess — issue #1376

## Scope

- Worktree: `D:/Dev/worktrees/openless/issue-1376-remove-keyboard-toggle`.
- Branch: `fix/issue-1376-remove-keyboard-toggle`.
- Base: upstream `Open-Less/openless` `beta`, fetched at `81241467c8f1c902bc529af9de83e7bbae2c993f`.
- Push target: `origin` = `HKLHaoBin/openless`, not upstream.
- Issue: `Open-Less/openless#1376`.
- Original checkout `D:/Dev/src/openless` stays on `fix/android-credential-dir`; its existing untracked research document is unrelated and must remain untouched.

## Verified architecture

- PR #1143 migrated the Windows text service from keyboard TIP to speech TIP. The speech profile remains enabled and does not appear in Win+Space.
- This change removes the obsolete settings toggle, handler/predicate and label/description/error in all eight frontend locales.
- Keep the legacy `windowsShowOpenlessInKeyboardList` wire field, defaults and cloud-sync representation for compatibility. Do not alter native registration, activation or insertion behavior.
- The shared settings provider already reports insertion-setting save failures; do not replace those with a keyboard-list-specific message.

## Validation and delivery contract

- Tests, CI and desktop/mobile artifact builds run only in GitHub Actions; no local test execution, dependency installation or product build for this task.
- Regression contract: `openless-all/app/scripts/windows-keyboard-list-removal.test.mjs`; existing insertion/newline predicate tests remain.
- Require an independent `review-diff-bugs` review of the complete uncommitted candidate, including tests, returning `no bug found` before commit/push. Re-review after any source/test/workflow repair.
- Dispatch branch-only CI (`platform=all`), Tauri desktop (`platform=all`, no release tag or diagnostics) and Android APK (`abis=all`, standard profile) after verifying remote HEAD.
- CI includes full Linux egui package validation; its standalone release workflow requires a release tag and must not be dispatched for this branch-only delivery.
- Watch each Actions run exactly once using `gh run watch --exit-status`. On failure retrieve the complete log and fix demonstrated in-scope causes before the next reviewed candidate.
- No PR, tag, release or upstream push is authorized.

## Status

Implementation candidate prepared. Review, remote tests and artifact acceptance remain pending until recorded by actual review and Actions results.
