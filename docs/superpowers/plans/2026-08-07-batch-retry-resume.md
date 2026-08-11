# Batch Retry and Resume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retry failed video compression once, continue remaining files, skip validated completed outputs, and expose every final failure on demand.

**Architecture:** Add a small per-file result model shared by the batch and auto workers. The Rust layer gains an FFprobe-backed output validation command; Vue workers use it to skip completed outputs and collect two-attempt failures; persisted task metadata stores counts and details for the expandable task UI.

**Tech Stack:** Vue 3, TypeScript, Tauri 2, Rust, FFmpeg/FFprobe.

## Global Constraints

- Attempt each file exactly twice at most.
- Skip only outputs that exist and contain a valid video stream.
- Preserve all existing encoding arguments and cancellation behavior.
- Continue processing siblings after a file fails.
- Detect input-to-output collisions before launching parallel work.
- Keep failure details hidden until a user expands the task.

---

### Task 1: Add output validation and task-detail data contracts

**Files:**
- Modify: `src-tauri/src/compress.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/compress.ts`
- Modify: `src/taskStore.ts`

- [ ] Add a Tauri command that accepts an output path and returns `true` only when it is a non-empty file with a readable video stream; use the existing FFprobe resolution probe and hidden-window subprocess configuration.
- [ ] Expose `isCompressedOutputValid(path)` in `src/compress.ts`.
- [ ] Add typed task metadata for `completedCount`, `skippedCount`, and `failures: Array<{ inputPath: string; message: string }>`; preserve it through SQLite task persistence.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml` and `cargo check --manifest-path src-tauri/Cargo.toml`.

### Task 2: Make manual batch jobs resumable and retryable

**Files:**
- Modify: `src/views/WorkbenchView.vue`
- Modify: `src/utils.ts`

- [ ] Before creating workers, derive every output path and reject any duplicate output path with a clear collision error.
- [ ] Before invoking compression, call `isCompressedOutputValid`; count valid outputs as skipped.
- [ ] For each non-skipped input, run compression once, retry exactly once on a non-cancellation error, and record the final error with its input path.
- [ ] Keep workers running after individual failures; persist completed/skipped/failure counts and details.
- [ ] Verify with mocked Tauri invokes that valid outputs skip compression, an initial failure retries once, two failures retain the final error, cancellation does not retry, and duplicate output paths abort before workers start.

### Task 3: Make automatic compression resumable per video

**Files:**
- Modify: `src/views/AutoCompressView.vue`
- Modify: `src/autoStore.ts`

- [ ] Reuse the output validation and two-attempt worker behavior for every video in a drama.
- [ ] Catch per-video failure and continue the worker pool; aggregate successes, skips, and all failure details.
- [ ] Mark a drama done only if every video is completed or validly skipped; leave failures eligible for the next scan without re-encoding valid output files.
- [ ] Verify a single failed video does not stop later videos and a later scan retries only that failed output.

### Task 4: Render expandable detailed errors in task history

**Files:**
- Modify: `src/views/TasksView.vue`
- Modify: `src/taskStore.ts`

- [ ] Keep the compact task state summary unchanged by default.
- [ ] Add an expand/collapse control only for tasks with failure details.
- [ ] When expanded, render every failed input filename/path and its final FFmpeg error in a readable scrollable list.
- [ ] Verify successful and skipped-only tasks do not display an error-details control, while a multi-file failed task displays every recorded failure.

### Task 5: Verify end-to-end behavior

**Files:**
- Modify: `docs/superpowers/specs/2026-08-07-batch-retry-resume-design.md`

- [ ] Run frontend type checking/build and Rust checks.
- [ ] Manually run a batch with an intentionally bad video plus valid videos; confirm valid files continue, the bad file is attempted twice, and all detail is visible only after expansion.
- [ ] Re-run the same batch; confirm valid outputs are skipped.
- [ ] On Windows, repeat with an auto-scan drama and confirm no console windows appear.
