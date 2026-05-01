# Issue #98 Tracking

Scope: Windows startup `visible` / `ready` lifecycle contract

Current stage:

- This branch is a draft PR placeholder.
- No runtime fix is included yet.
- The goal is to freeze the problem boundary before implementation.

Problem statement:

- The main window can become visible before hotkey/runtime readiness is truly established.
- Backend and frontend both participate in first-show ownership.
- Windows is the currently observed symptom surface, but the ownership split is architectural.

Implementation target to converge before coding:

- Decide a single owner for initial main-window visibility.
- Define the relation between `created`, `shown`, `startup`, `ready`, and `first usable state`.
- Decide whether startup shell and ready shell remain separate concepts.

Non-goals in this draft:

- No Tauri window behavior change yet
- No frontend gate refactor yet
- No smoke implementation yet
