# Auto Compress In-Place Incremental Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Auto-compress replaces each source video in place (same filename), records per-drama video names in `auto-state.json`, and only batch-compresses newly added videos for recorded dramas whose folder is ≤2 days old.

**Architecture:** Pure TypeScript scan/diff helpers decide which videos to enqueue. Rust `list_drama_folders` exposes folder creation time; `compress_video` writes a sibling temp file then atomically replaces the source (killing holders if needed). `AutoRecord` gains `videoNames`/`failures`; auto UI stops using `影工输出/` and disables output-validity skip when input===output so source files are never treated as “already done.”

**Tech Stack:** Vue 3, TypeScript, Tauri 2, Rust, FFmpeg/FFprobe. Frontend unit tests: `node --experimental-strip-types --test src/<file>.test.ts`.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-08-12-auto-compress-inplace-incremental-design.md`
- New dramas (no `done` record): always full-folder enqueue regardless of age.
- Old dramas: scan video details only when folder age ≤ 2 days; enqueue only names not in `videoNames`.
- Folder age: birth → ctime → mtime fallback (fixed order).
- In-place: temp `.影工临时_<stem>_<id>.<ext>` → validate → replace; on lock kill holders (never self PID); failure reason must include `文件被占用`.
- Do not create `影工输出` for auto compress; do not use `toBatchName` on the auto path.
- Workbench manual compress unchanged (`_batch` + user output dir).
- Encoding presets/args unchanged.
- Auto batch must **not** skip via `isCompressedOutputValid` when `inputPath === outputPath` (would skip every source). Resume/skip is via `videoNames` only.
- Exclude temp files matching `.影工临时_*` from drama video listing.

---

## File structure

| File | Responsibility |
|------|----------------|
| `src/autoScan.ts` | Pure helpers: freshness, pending videos, legacy seed |
| `src/autoScan.test.ts` | Unit tests for scan/diff |
| `src/types.ts` | `DramaFolder.createdAtMs`, `AutoRecord` extensions |
| `src/autoPersistence.ts` | Upsert merge of `videoNames` / `failures` |
| `src/autoStore.ts` | Store APIs for upsert / seed |
| `src-tauri/src/persist.rs` | Serde structs for new fields |
| `src-tauri/src/media.rs` | `created_at_ms`, skip temp videos |
| `src-tauri/src/replace.rs` | Temp name, atomic replace, kill lock holders |
| `src-tauri/src/compress.rs` | In-place encode path when output==input |
| `src-tauri/src/lib.rs` | Register module if needed |
| `src/views/AutoCompressView.vue` | Scan rules, enqueue, in-place queue, UI copy |
| `README.md` | One-line auto-compress description |

---

### Task 1: Scan/diff helpers (pure TS)

**Files:**
- Create: `src/autoScan.ts`
- Create: `src/autoScan.test.ts`

**Interfaces:**
- Consumes: `DramaFolder` / `AutoRecord` shapes from `src/types.ts` (extend in Task 2; for this task define local minimal types or import and update types first in Step 1).
- Produces:
  - `export const TWO_DAYS_MS = 2 * 24 * 60 * 60 * 1000`
  - `export function isDramaFolderFresh(createdAtMs: number, nowMs: number, maxAgeMs?: number): boolean`
  - `export function pendingVideosForDrama(folder: { videos: { name: string }[] }, record: { videoNames?: string[] } | null | undefined): { name: string }[]`
  - `export function shouldMonitorOldDrama(createdAtMs: number, nowMs: number): boolean` — alias of fresh check for old dramas
  - `export function seedVideoNames(folder: { videos: { name: string }[] }): string[]`

- [ ] **Step 1: Extend types first (minimal)**

In `src/types.ts`:

```ts
export type DramaFolder = {
  name: string
  path: string
  videoCount: number
  /** Unix ms folder birth/ctime/mtime from Rust; 0 if unknown */
  createdAtMs: number
  videos: { name: string; path: string; size: number }[]
}

export type AutoVideoFailure = {
  name: string
  reason: string
  at: string
}

export type AutoRecord = {
  path: string
  name: string
  completedAt: string
  videoCount: number
  videoNames?: string[]
  failures?: AutoVideoFailure[]
}
```

- [ ] **Step 2: Write failing tests**

`src/autoScan.test.ts`:

```ts
/// <reference types="node" />
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  TWO_DAYS_MS,
  isDramaFolderFresh,
  pendingVideosForDrama,
  seedVideoNames,
} from './autoScan.ts'

test('folder newer than 2 days is fresh', () => {
  const now = 1_700_000_000_000
  assert.equal(isDramaFolderFresh(now - TWO_DAYS_MS + 1, now), true)
  assert.equal(isDramaFolderFresh(now - TWO_DAYS_MS - 1, now), false)
})

test('new drama (null record) returns all videos', () => {
  const folder = { videos: [{ name: 'a.mp4' }, { name: 'b.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, null).map((v) => v.name),
    ['a.mp4', 'b.mp4'],
  )
})

test('old drama pending excludes successful names', () => {
  const folder = { videos: [{ name: 'a.mp4' }, { name: 'b.mp4' }, { name: 'c.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, { videoNames: ['a.mp4', 'c.mp4'] }).map((v) => v.name),
    ['b.mp4'],
  )
})

test('missing videoNames treated as empty for pending (caller seeds separately)', () => {
  const folder = { videos: [{ name: 'a.mp4' }] }
  assert.deepEqual(
    pendingVideosForDrama(folder, { videoNames: undefined }).map((v) => v.name),
    ['a.mp4'],
  )
})

test('seedVideoNames lists all current names', () => {
  assert.deepEqual(seedVideoNames({ videos: [{ name: 'x.mp4' }, { name: 'y.mp4' }] }), [
    'x.mp4',
    'y.mp4',
  ])
})
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `node --experimental-strip-types --test src/autoScan.test.ts`  
Expected: FAIL module not found / export missing.

- [ ] **Step 4: Implement `src/autoScan.ts`**

```ts
export const TWO_DAYS_MS = 2 * 24 * 60 * 60 * 1000

export function isDramaFolderFresh(
  createdAtMs: number,
  nowMs: number,
  maxAgeMs: number = TWO_DAYS_MS,
): boolean {
  if (!Number.isFinite(createdAtMs) || createdAtMs <= 0) return false
  return nowMs - createdAtMs <= maxAgeMs
}

export function shouldMonitorOldDrama(createdAtMs: number, nowMs: number): boolean {
  return isDramaFolderFresh(createdAtMs, nowMs)
}

export function pendingVideosForDrama<T extends { name: string }>(
  folder: { videos: T[] },
  record: { videoNames?: string[] } | null | undefined,
): T[] {
  const done = new Set(record?.videoNames ?? [])
  return folder.videos.filter((v) => !done.has(v.name))
}

export function seedVideoNames(folder: { videos: { name: string }[] }): string[] {
  return folder.videos.map((v) => v.name)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `node --experimental-strip-types --test src/autoScan.test.ts`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/types.ts src/autoScan.ts src/autoScan.test.ts
git commit -m "$(cat <<'EOF'
添加自动压制扫描差分辅助函数

按剧目年龄与已成功视频名计算待压列表，并扩展 AutoRecord 类型。
EOF
)"
```

---

### Task 2: Persist upsert for videoNames / failures

**Files:**
- Modify: `src-tauri/src/persist.rs`
- Modify: `src/autoPersistence.ts`
- Modify: `src/autoStore.ts`
- Modify: `src/autoStore.test.ts`

**Interfaces:**
- Consumes: `AutoRecord` from Task 1
- Produces:
  - `publishUpsertDramaRecord(records, record, ops)` — merge by `path`
  - `markDramaVideosDone` / `recordDramaFailures` on autoStore (names below)

- [ ] **Step 1: Update Rust `AutoRecord`**

In `src-tauri/src/persist.rs`:

```rust
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoVideoFailure {
  pub name: String,
  pub reason: String,
  pub at: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoRecord {
  pub path: String,
  pub name: String,
  pub completed_at: String,
  pub video_count: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub video_names: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub failures: Option<Vec<AutoVideoFailure>>,
}
```

Keep load/save commands unchanged (serde handles new fields).

- [ ] **Step 2: Write failing persistence tests**

Add to `src/autoStore.test.ts` (or new cases in same file):

```ts
import { publishUpsertDramaRecord } from './autoPersistence.ts'

test('upsert merges videoNames and clears matching failures', async () => {
  const existing = [
    {
      path: '/d/a',
      name: 'A',
      completedAt: 't0',
      videoCount: 1,
      videoNames: ['1.mp4'],
      failures: [{ name: '2.mp4', reason: '文件被占用', at: 't0' }],
    },
  ]
  let published = existing
  await publishUpsertDramaRecord(
    existing,
    {
      path: '/d/a',
      name: 'A',
      completedAt: 't1',
      videoCount: 2,
      videoNames: ['1.mp4', '2.mp4'],
      failures: [],
    },
    {
      persist: async () => {},
      publish: (r) => {
        published = r
      },
    },
  )
  const row = published.find((r) => r.path === '/d/a')!
  assert.deepEqual(row.videoNames, ['1.mp4', '2.mp4'])
  assert.deepEqual(row.failures, [])
  assert.equal(row.videoCount, 2)
})

test('upsert inserts when path missing', async () => {
  let published: typeof existing = []
  const existing: Array<{
    path: string
    name: string
    completedAt: string
    videoCount: number
    videoNames?: string[]
  }> = []
  await publishUpsertDramaRecord(
    existing,
    {
      path: '/d/new',
      name: 'New',
      completedAt: 't',
      videoCount: 1,
      videoNames: ['a.mp4'],
    },
    { persist: async () => {}, publish: (r) => { published = r } },
  )
  assert.equal(published[0]?.path, '/d/new')
})
```

Keep existing `publishPersistedDramaDone` tests working (leave function for compatibility or thin-wrap upsert insert-only if still used).

- [ ] **Step 3: Run tests — expect fail**

Run: `node --experimental-strip-types --test src/autoStore.test.ts`  
Expected: FAIL missing `publishUpsertDramaRecord`.

- [ ] **Step 4: Implement upsert**

`src/autoPersistence.ts`:

```ts
import type { AutoRecord } from './types.ts'

export async function publishPersistedDramaDone(/* keep existing signature/behavior */) { /* unchanged */ }

export async function publishUpsertDramaRecord(
  records: AutoRecord[],
  record: AutoRecord,
  operations: {
    persist: (records: AutoRecord[]) => Promise<unknown>
    mirror?: (records: AutoRecord[]) => void
    onMirrorError?: (error: unknown) => void
    publish: (records: AutoRecord[]) => void
  },
) {
  const idx = records.findIndex((item) => item.path === record.path)
  const next =
    idx === -1
      ? [record, ...records]
      : records.map((item, i) => (i === idx ? record : item))
  await operations.persist(next)
  try {
    operations.mirror?.(next)
  } catch (error) {
    operations.onMirrorError?.(error)
  }
  operations.publish(next)
}
```

In `src/autoStore.ts` add:

```ts
export async function upsertAutoRecord(record: AutoRecord) {
  await publishUpsertDramaRecord(autoDone.value, record, {
    persist: (done) =>
      persistCanonicalAutoState({
        watchDir: autoWatchDir.value,
        enabled: autoEnabled.value,
        done,
      }),
    mirror: (done) =>
      mirrorAutoState({
        watchDir: autoWatchDir.value,
        enabled: autoEnabled.value,
        done,
      }),
    onMirrorError: (err) => {
      console.error('同步自动压制调试记录失败', err)
    },
    publish: (done) => {
      autoDone.value = done
    },
  })
  await nextTick()
  if (persistTimer) {
    window.clearTimeout(persistTimer)
    persistTimer = undefined
  }
}

/** Merge successful names into existing record (or create). */
export async function appendDramaVideoSuccess(args: {
  path: string
  name: string
  videoName: string
  at: string
}) {
  const prev = autoDone.value.find((r) => r.path === args.path)
  const names = new Set(prev?.videoNames ?? [])
  names.add(args.videoName)
  const videoNames = [...names]
  const failures = (prev?.failures ?? []).filter((f) => f.name !== args.videoName)
  await upsertAutoRecord({
    path: args.path,
    name: args.name,
    completedAt: args.at,
    videoCount: videoNames.length,
    videoNames,
    failures: failures.length ? failures : [],
  })
}

export async function recordDramaVideoFailure(args: {
  path: string
  name: string
  videoName: string
  reason: string
  at: string
}) {
  const prev = autoDone.value.find((r) => r.path === args.path)
  if (!prev?.videoNames?.length) {
    // Spec: never create done-only-from-failures for brand-new dramas.
    // Still update failures if record already exists.
    if (!prev) return
  }
  const failures = [
    ...(prev.failures ?? []).filter((f) => f.name !== args.videoName),
    { name: args.videoName, reason: args.reason, at: args.at },
  ]
  await upsertAutoRecord({
    ...prev,
    failures,
  })
}

export async function seedDramaVideoNames(args: {
  path: string
  name: string
  videoNames: string[]
  at: string
}) {
  await upsertAutoRecord({
    path: args.path,
    name: args.name,
    completedAt: args.at,
    videoCount: args.videoNames.length,
    videoNames: args.videoNames,
    failures: [],
  })
}
```

Adjust `recordDramaVideoFailure`: if record exists, update failures; if no record yet, allow AutoCompressView to pass failures only after first success path, OR store failures on an in-memory job until first success — prefer: **if `prev` missing, skip persist** (failures still go to task meta). After first success, failures append works.

- [ ] **Step 5: Pass tests + `cargo check`**

Run:
```bash
node --experimental-strip-types --test src/autoStore.test.ts
cargo check --manifest-path src-tauri/Cargo.toml
```
Expected: PASS / OK

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/persist.rs src/autoPersistence.ts src/autoStore.ts src/autoStore.test.ts
git commit -m "$(cat <<'EOF'
支持自动压制按视频名 upsert 持久化

扩展 auto-state 的 videoNames/failures，并提供成功追加与失败记录 API。
EOF
)"
```

---

### Task 3: Rust folder `createdAtMs` + skip temp videos

**Files:**
- Modify: `src-tauri/src/media.rs`
- Modify: `src/drama.ts` (if needed — types already updated)

**Interfaces:**
- Produces: `DramaFolder.created_at_ms: u64` (serde camelCase `createdAtMs`)

- [ ] **Step 1: Add helper + field**

```rust
fn folder_created_at_ms(path: &Path) -> u64 {
  let meta = match fs::metadata(path) {
    Ok(m) => m,
    Err(_) => return 0,
  };
  if let Ok(t) = meta.created() {
    return system_time_to_ms(t);
  }
  // ctime: not portable on all platforms via std; use modified as next fallback
  // On Unix, prefer metadata.modified() only after created(); document birth→mtime.
  // Optional: use filetime/libc for ctime — if adding deps is undesirable, use:
  // created() then modified() (birth → mtime). Spec allows ctime in the middle when available.
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    let ctime_ms = (meta.ctime() as u64).saturating_mul(1000);
    if ctime_ms > 0 {
      return ctime_ms;
    }
  }
  if let Ok(t) = meta.modified() {
    return system_time_to_ms(t);
  }
  0
}

fn system_time_to_ms(t: std::time::SystemTime) -> u64 {
  t.duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}
```

Update `DramaFolder`:

```rust
pub struct DramaFolder {
  pub name: String,
  pub path: String,
  pub video_count: usize,
  pub created_at_ms: u64,
  pub videos: Vec<DramaVideo>,
}
```

When pushing folders, set `created_at_ms: folder_created_at_ms(&path)`.

In `collect_videos`, skip names starting with `.影工临时_` (and existing hidden/dot rules).

- [ ] **Step 2: Compile check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`  
Expected: OK (frontend may need default `createdAtMs: 0` in any mocks).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/media.rs src/types.ts
git commit -m "$(cat <<'EOF'
剧目扫描返回目录创建时间并忽略临时输出

为自动压制 2 天窗口与增量扫描提供 createdAtMs。
EOF
)"
```

---

### Task 4: In-place replace + kill lock holders (Rust)

**Files:**
- Create: `src-tauri/src/replace.rs`
- Modify: `src-tauri/src/compress.rs`
- Modify: `src-tauri/src/lib.rs` (`mod replace;`)

**Interfaces:**
- Produces:
  - `pub fn temp_output_path(final_path: &Path) -> PathBuf`
  - `pub fn replace_file_force(temp: &Path, final_path: &Path) -> Result<(), String>`
  - `compress_video`: if `output_path == input`, encode to temp then `replace_file_force`

- [ ] **Step 1: Implement `replace.rs`**

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn temp_output_path(final_path: &Path) -> PathBuf {
  let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
  let stem = final_path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
  let ext = final_path
    .extension()
    .and_then(|s| s.to_str())
    .map(|e| format!(".{e}"))
    .unwrap_or_default();
  let id = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or(0);
  parent.join(format!(".影工临时_{stem}_{id}{ext}"))
}

fn pids_holding(path: &Path) -> Vec<u32> {
  let path_str = path.to_string_lossy();
  let output = Command::new("lsof")
    .args(["-t", "--", path_str.as_ref()])
    .output();
  let Ok(out) = output else { return vec![] };
  if !out.status.success() && out.stdout.is_empty() {
    return vec![];
  }
  String::from_utf8_lossy(&out.stdout)
    .lines()
    .filter_map(|l| l.trim().parse::<u32>().ok())
    .filter(|pid| *pid != std::process::id())
    .collect()
}

fn kill_pid(pid: u32) {
  #[cfg(unix)]
  {
    let _ = Command::new("kill").args([pid.to_string()]).output();
  }
  #[cfg(windows)]
  {
    let _ = Command::new("taskkill")
      .args(["/PID", &pid.to_string(), "/F"])
      .output();
  }
}

pub fn replace_file_force(temp: &Path, final_path: &Path) -> Result<(), String> {
  // try rename/replace first
  if try_replace(temp, final_path).is_ok() {
    return Ok(());
  }
  let pids = pids_holding(final_path);
  for pid in &pids {
    kill_pid(*pid);
  }
  // brief pause
  std::thread::sleep(std::time::Duration::from_millis(200));
  match try_replace(temp, final_path) {
    Ok(()) => Ok(()),
    Err(e) => {
      let _ = fs::remove_file(temp);
      Err(format!("文件被占用，无法替换原文件：{e}"))
    }
  }
}

fn try_replace(temp: &Path, final_path: &Path) -> Result<(), String> {
  // On Windows, remove destination first if needed
  if final_path.exists() {
    fs::remove_file(final_path).map_err(|e| e.to_string())?;
  }
  fs::rename(temp, final_path).map_err(|e| e.to_string())
}
```

Do **not** add a `libc` dependency. On Unix kill with:

```rust
let _ = Command::new("kill").args([pid.to_string()]).output();
```

- [ ] **Step 2: Wire `compress_video` in-place path**

After resolving `output_path`, if `output_path == input`:

1. `let temp = temp_output_path(&output_path);`
2. Run existing ffmpeg loop writing to `temp` (not `output_path`).
3. On success + validation against `temp`, call `replace_file_force(&temp, &output_path)`.
4. On cancel/fail, `remove_file(temp)`.
5. Return `CompressResult` with `output_path` = final path.

If `output_path != input`, keep current behavior (direct write).

- [ ] **Step 3: Add a focused Rust unit test for temp naming** (optional but preferred)

```rust
#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn temp_name_is_hidden_sibling() {
    let p = PathBuf::from("/Movies/剧/01.mp4");
    let t = temp_output_path(&p);
    let name = t.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(".影工临时_01_"));
    assert!(name.ends_with(".mp4"));
    assert_eq!(t.parent(), p.parent());
  }
}
```

Run: `cargo test --manifest-path src-tauri/Cargo.toml temp_name_is_hidden_sibling`  
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/replace.rs src-tauri/src/compress.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "$(cat <<'EOF'
压缩支持原位临时写出并强制替换

编码写入隐藏临时文件，替换前结束占用进程，失败保留源文件。
EOF
)"
```

---

### Task 5: AutoCompressView scan, enqueue, in-place job

**Files:**
- Modify: `src/views/AutoCompressView.vue`
- Modify: `README.md` (auto row)

**Interfaces:**
- Consumes: `autoScan` helpers, `upsert`/`append`/`seed` from autoStore, `createdAtMs` from folders
- Produces: working auto UI behavior per spec

- [ ] **Step 1: Replace skip/enqueue logic**

Remove whole-drama `isDramaDone` short-circuit as the only gate. New helpers inside the view (or imported):

```ts
import {
  pendingVideosForDrama,
  seedVideoNames,
  shouldMonitorOldDrama,
} from '../autoScan'
import {
  appendDramaVideoSuccess,
  recordDramaVideoFailure,
  seedDramaVideoNames,
  autoDone,
  // ...
} from '../autoStore'

function recordFor(path: string) {
  return autoDone.value.find((r) => r.path === path)
}

function videosToProcess(folder: DramaFolder) {
  const rec = recordFor(folder.path)
  const now = Date.now()
  if (!rec) {
    return folder.videos // new drama: all
  }
  // legacy seed
  if (!rec.videoNames) {
    return null // signal: seed only, no compress this pass
  }
  if (!shouldMonitorOldDrama(folder.createdAtMs, now)) {
    return [] // too old: skip
  }
  return pendingVideosForDrama(folder, rec)
}

function shouldSkipDrama(path: string) {
  const folder = folders.value.find((f) => f.path === path)
  if (!folder) return true
  const pending = videosToProcess(folder)
  if (pending === null) return false // need seed pass — still "process" in scan
  if (pending.length === 0) return true
  // also skip if already queued/running locally or in tasks (keep existing task checks)
  ...
}
```

In `scanNow` / `enqueuePending`:
- If `videosToProcess === null`: call `seedDramaVideoNames({ path, name, videoNames: seedVideoNames(folder), at: localeString })` and do **not** enqueue compress.
- Else if pending length > 0: enqueue job with `videoCount: pending.length` and store pending video list on the job (extend `DramaJob` with `videos: DramaFolder['videos']`).

- [ ] **Step 2: Change `submitDrama` output paths**

```ts
const outputDir = folder.path // drama folder itself
const videos = job.videos // pending only
const queue = videos.map((video) => {
  const outputName = video.name // same filename, NOT toBatchName
  return {
    id: makeId(),
    inputPath: video.path,
    outputDir,
    outputName,
    outputPath: toOutputPath(outputDir, outputName), // === inputPath
  }
})

const result = await runAutoDramaJob(queue, getCompressConcurrency(), {
  prepareBatch: () => prepareCompressBatch(taskId),
  // CRITICAL: never skip inplace sources via ffprobe validity
  isOutputValid: async () => false,
  compress: (item, onProgress) =>
    compressVideo({
      id: item.id,
      cancelKey: taskId,
      inputPath: item.inputPath,
      outputDir: item.outputDir,
      outputName: item.outputName,
      onProgress,
    }),
  ...
})
```

After each successful file: ideally hook progress — if batch API only reports at end, then after `result`:
- For each queue item not in `result.meta.failures`, `appendDramaVideoSuccess`.
- For each failure, `recordDramaVideoFailure` with `reason` from message (ensure occupied errors already contain `文件被占用`).

If all fail and no prior `videoNames`, do not create a done record (append only runs on success).

Task completion: `completeTask` if no failures; `failTask` if any failures (keep successes in `videoNames`).

- [ ] **Step 3: Update copy**

Header/hint:

```html
每个子文件夹视为一个剧目。压制成功后原位替换同名文件；已记录剧目在目录创建 2 天内会增量压制新增视频。
```

Remove `监控目录/影工输出/剧目名/` text.

README table row for 自动压制: say in-place replace + incremental within 2 days.

- [ ] **Step 4: Done list (minimal expand)**

If there is no separate done list UI today, add a compact section under the queue OR enrich the summary line. Spec asks: show drama name + success count; expand for names/failures. Minimal approach — under controls after summary:

```html
<ul v-if="autoDone.length" class="file-list">
  <li v-for="r in autoDone" :key="r.path" class="file-item">
    <div class="file-meta">
      <p class="file-name">{{ r.name }}</p>
      <p class="file-sub">已成功 {{ r.videoNames?.length ?? r.videoCount }} 个</p>
      <details v-if="(r.videoNames?.length || r.failures?.length)">
        <summary>明细</summary>
        <p v-for="n in r.videoNames || []" :key="n">{{ n }}</p>
        <p v-for="f in r.failures || []" :key="f.name + f.at" class="err">
          {{ f.name }}：{{ f.reason }}
        </p>
      </details>
    </div>
  </li>
</ul>
```

- [ ] **Step 5: Typecheck**

Run: `pnpm exec vue-tsc -b --pretty false`  
Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add src/views/AutoCompressView.vue README.md
git commit -m "$(cat <<'EOF'
自动压制改为原位替换并支持增量补压

按剧目记录视频名，2 天内旧剧目只压制新增文件，不再输出到影工输出。
EOF
)"
```

---

### Task 6: Verification pass

**Files:**
- None required unless fixes found

- [ ] **Step 1: Run unit tests**

```bash
node --experimental-strip-types --test src/autoScan.test.ts src/autoStore.test.ts src/utils.test.ts src/manualBatch.test.ts
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all PASS.

- [ ] **Step 2: Manual checklist (desktop)**

1. New drama folder with 2 videos → both replaced in place; `videoNames` has both; no `影工输出`.
2. Add a third video while folder age ≤2 days → only third enqueued.
3. Drama older than 2 days with new file → no enqueue.
4. Legacy record without `videoNames` → seed all names, no recompress.
5. Open a video in a player during replace → process killed or failure recorded with `文件被占用`.
6. Workbench still writes `_batch` to chosen output dir.

- [ ] **Step 3: Commit any fixes** (if needed) with a focused message.

---

## Spec coverage self-check

| Spec requirement | Task |
|------------------|------|
| In-place replace same filename | 4, 5 |
| Temp then atomic replace; kill holders; occupied error | 4 |
| Record drama + video names | 2, 5 |
| New drama full folder | 1, 5 |
| Old drama ≤2d incremental | 1, 3, 5 |
| Old drama >2d skip details | 1, 5 |
| Legacy seed without recompress | 1, 5 |
| No `影工输出` for auto | 5 |
| Failures + partial persist | 2, 5 |
| UI copy + expand details | 5 |
| Workbench unchanged | 4 (branch), 5 (scope) |
| Skip temp files in listing | 3 |
| Do not use isOutputValid skip for inplace | 5 |
