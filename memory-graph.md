# Memory Graph — OpenLess ASR polish strategy

- slug: openless-polish-asr-llm
- path: D:/Dev/worktrees/openless/polish-asr-llm
- updated: 2026-10-27

## Summary

OpenLess PR #1262 adds optional, default-off Rhai policies between final corrected ASR and ordinary LLM polishing. This worktree uses branch `HKLHaoBin/polish-asr-llm`; upstream base is `Open-Less/openless:beta`.

## Entities

- OpenLess (Repo): upstream `https://github.com/Open-Less/openless`; fork remote `origin` is `HKLHaoBin/openless`.
- openless-core (Service): Rust pipeline, bounded Rhai evaluation, request overrides and preferences.
- React settings (Service): strategy editing and copying authoring instructions with the current draft.
- Linux egui settings (Service): native strategy configuration.

## Relations

- dictation_engine --evaluates--> polish_strategy: after ASR correction and final transcript publication.
- polish_strategy --selects--> ordinary polish: skip, polish or scoped request overrides; invalid scripts fall back.
- strategy metadata --belongs-to--> winning ASR invocation: unavailable values must not leak from a losing cloud route.

## Facts

- Main Rust workspace and frontend: `openless-all/app`.
- Core tests: `cargo +1.99.0 test --locked -p openless-core --lib` from that directory.
- Frontend build/test: `npm test`; focused authoring contract: `node --import tsx src/lib/polishStrategyAgentSkill.test.ts`.
- PR workflows include `CI` and `iOS tests and device build`; validate runs against the exact pushed head rather than old passing runs.

## Decisions

- Integrate beta using merge commits; do not rewrite the published branch history.
- Preserve translation, Voice Edit, cloud notes, QA and short-input threshold semantics.
- Strategy sandbox has no file, network or process access; no automatic Agent invocation or script overwrite.
- Review/fix only bugs both in the selected diff's changed operations and within those operations' responsibility; unrelated upstream bugs are outside this PR.
