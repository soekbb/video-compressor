# Deep Drama Folder Scan Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scan watch trees to any depth and register only deepest directories that contain videos as dramas, named by relative path.

**Architecture:** Extend `list_drama_folders` in Rust to recursively collect directories whose own layer has videos, then drop any candidate that has a deeper candidate under it. Frontend only updates copy; `DramaFolder` shape stays the same.

**Tech Stack:** Rust (Tauri), Vue copy tweak, `cargo test`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-14-auto-compress-deep-drama-folders-design.md`
- Prefer deeper: drop ancestor dramas when a descendant drama exists
- `name` = relative path with `/`; `path` absolute
- Max depth 8 from watch root; never register the watch root itself
- Skip dir names: `.…`, `影工输出`, `快压输出`, `_compressed`
- Videos still collected only at the drama’s own layer

---

### Task 1: Recursive drama discovery + tests

**Files:**
- Modify: `src-tauri/src/media.rs`
- Modify: `src/views/AutoCompressView.vue` (hint copy only)
- Modify: `README.md` if it still says 一级子文件夹

- [ ] **Step 1: Write failing tests** for relative naming and ancestor drop (temp dirs with nested mp4 stubs).

- [ ] **Step 2: Implement** `should_skip_dir_name`, recursive walk, ancestor filter, wire `list_drama_folders`.

- [ ] **Step 3: Run** `cargo test --manifest-path src-tauri/Cargo.toml --lib`

- [ ] **Step 4: Update UI copy** — 不再写「每个子文件夹」，改为任意深度本层有视频的目录。

- [ ] **Step 5: Commit**
