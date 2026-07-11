# Code Quality Cleanup Roadmap

> From: 1-app audit (2026-06-17)
> Issue: #699
> Author: zsxh1990

## Overview

This document tracks the code quality cleanup items identified in the security audit. Each item has a specific action, risk level, and estimated effort.

## Items

### 1. Remove blanket `#![allow(dead_code, unused_imports, unused_variables)]` (#27)

**Files affected:**
- `src/lib.rs` (line 1-4)
- `src/coordinator.rs` (line 1-4)

**Current state:**
```rust
#![cfg_attr(
    target_os = "linux",
    allow(dead_code, unused_imports, unused_variables)
)]
```

**Action:** Remove the blanket allow and clean dead code incrementally.

**Approach:**
1. Remove the blanket allow from both files
2. Run `cargo check` to identify all warnings
3. Fix warnings in priority order:
   - Remove unused imports
   - Remove unused variables (prefix with `_`)
   - Remove dead code or mark with targeted `#[allow(dead_code)]`

**Risk:** Low (no behavior change, only compiler warnings)
**Effort:** 2-4 hours
**Dependencies:** None

### 2. Extract shared request builder in polish.rs (#24)

**File affected:** `src/polish.rs`

**Current state:**
- `OpenAICompatibleLLMProvider` (line 278) implements `polish()`, `translate_to()`, `answer_chat_streaming()`
- `CodexOAuthLLMProvider` (line 912) implements the same functions
- Both share ~10-arg function signatures and similar logic

**Action:** Extract a shared `RequestBuilder` trait or struct.

**Approach:**
1. Identify common parameters across providers
2. Create `RequestBuilder` struct with shared fields
3. Implement builder pattern for provider-specific options
4. Refactor providers to use shared builder

**Risk:** Medium (refactor, but no behavior change)
**Effort:** 4-6 hours
**Dependencies:** None

### 3. Rename misleading `mobile_stubs/selection.rs` (#32)

**File affected:** `src/mobile_stubs/selection.rs`

**Current state:**
- File is labeled as "stubs" but contains real implementation
- 54 lines of actual code

**Action:** Rename to `src/selection_impl.rs` or move to `src/selection/mobile.rs`

**Approach:**
1. Check if file is actually used (grep for imports)
2. Rename file and update imports
3. Update any documentation references

**Risk:** Low (rename only)
**Effort:** 30 minutes
**Dependencies:** None

## Execution Order

1. **#27** (Remove blanket allow) — Lowest risk, immediate value
2. **#32** (Rename stub) — Quick win, improves code clarity
3. **#24** (Extract shared builder) — Most complex, do last

## Verification

After each change:
1. `cargo check` — No new warnings
2. `cargo test` — All tests pass
3. `cargo clippy` — No new lint warnings

## Notes

- The blanket allow is Linux-specific (`cfg_attr(target_os = "linux")`)
- Dead code warnings (~87) are likely Linux-only platform abstractions
- Consider adding targeted `#[cfg(target_os = "linux")]` instead of blanket allow
