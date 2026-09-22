# 压制按需 H.264 / moov 前置 / 默认分辨率 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 压制只在需要时动手：已是 H.264、目标分辨率且 moov 前置则跳过；只差 moov 在尾则流拷贝 faststart；否则转 H.264 并黑边对齐到设置中的 720p/1080p。合成仅在分集分辨率不一致时对齐到合成默认画幅。输出都 moov 前置。

**Architecture:** 新建 `output_spec.rs` 放分辨率档位、目标画幅、letterbox 滤镜、atom 探测和压制决策。`compress_video` 按决策跳过/重排/重编码。`merge_videos` 在分辨率不一致时用合成档位+多数派横竖作为 `target_wh`，filter 改为 scale+pad。设置两列存 SQLite 并在系统设置页选择。

**Tech Stack:** Rust / FFmpeg / Vue / rusqlite / `cargo test --lib` / `node --experimental-strip-types --test`

## Global Constraints

- 压制默认分辨率 720p，合成默认分辨率 1080p；取值仅 `720` | `1080`
- 输出一律 moov 前置（保留 `-movflags +faststart`）
- 已是 H.264 且宽高已是目标画幅且 moov 前置 → 跳过（原地不改文件；输出到其它目录则复制）
- 已是 H.264 且宽高已是目标画幅且 moov 后置 → `-c copy -movflags +faststart`
- 其它 → 重编码 H.264 + 黑边对齐目标；小于目标也放大
- 比例不对：不裁切、不拉伸，居中黑边
- 只重排与重编码都 `-map 0:v:0 -map 0:a:0?`
- 合成分辨率已一致：不强制 720/1080，快路径不变，输出保持 `+faststart`
- 合成分辨率不一致：统一到合成默认画幅，不再用第一集尺寸
- 横竖只看像素宽高（高>宽为竖），不单独解旋转矩阵
- 取消仍用现有 `cancel_key`
- 设置只影响新任务

**Spec:** `docs/superpowers/specs/2026-09-21-compress-moov-h264-resolution-design.md`

## File structure

- Create: `src-tauri/src/output_spec.rs` — 档位、画幅、letterbox、moov 探测、`CompressAction`
- Modify: `src-tauri/src/lib.rs` — `mod output_spec`
- Modify: `src-tauri/src/compress.rs` — 决策分支、map、目标分辨率校验
- Modify: `src-tauri/src/media.rs` — 不一致时目标画幅 + letterbox filter
- Modify: `src-tauri/src/tasks.rs` — 设置列与读写
- Modify: `src/types.ts`、`src/settings.ts`、`src/views/SettingsView.vue`、`src/compress.ts`、`src/merge.ts`、`src/App.vue`
- Test: `src-tauri/src/output_spec.rs`、`src-tauri/src/tasks.rs`、`src-tauri/src/media.rs`、`src/settings.test.ts`

---

### Task 1: 分辨率档位与目标画幅

**Files:**
- Create: `src-tauri/src/output_spec.rs`
- Modify: `src-tauri/src/lib.rs`（`mod compress;` 旁加 `mod output_spec;`）

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ResolutionPreset { P720, P1080 }`
  - `pub fn parse_resolution_preset(value: &str, fallback: ResolutionPreset) -> ResolutionPreset`
  - `pub fn target_canvas(preset: ResolutionPreset, width: u32, height: u32) -> (u32, u32)`
  - `pub fn letterbox_vf(target_w: u32, target_h: u32) -> String`
  - `pub fn majority_portrait(sizes: &[(u32, u32)]) -> bool`

- [ ] **Step 1: Write the failing test**

在 `output_spec.rs` 写模块和测试（函数先不写，让编译失败）：

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_resolution_preset_accepts_720_1080() {
    assert_eq!(
      parse_resolution_preset("720", ResolutionPreset::P1080),
      ResolutionPreset::P720
    );
    assert_eq!(
      parse_resolution_preset("1080", ResolutionPreset::P720),
      ResolutionPreset::P1080
    );
  }

  #[test]
  fn parse_resolution_preset_falls_back() {
    assert_eq!(
      parse_resolution_preset("4k", ResolutionPreset::P720),
      ResolutionPreset::P720
    );
    assert_eq!(
      parse_resolution_preset("", ResolutionPreset::P1080),
      ResolutionPreset::P1080
    );
    assert_eq!(
      parse_resolution_preset("720p", ResolutionPreset::P1080),
      ResolutionPreset::P1080
    );
  }

  #[test]
  fn target_canvas_portrait_and_landscape() {
    assert_eq!(
      target_canvas(ResolutionPreset::P720, 1080, 1920),
      (720, 1280)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 1080, 1920),
      (1080, 1920)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P720, 1920, 1080),
      (1280, 720)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 1920, 1080),
      (1920, 1080)
    );
    assert_eq!(
      target_canvas(ResolutionPreset::P1080, 100, 100),
      (1920, 1080)
    );
  }

  #[test]
  fn letterbox_vf_fits_inside_and_pads() {
    let vf = letterbox_vf(720, 1280);
    assert!(vf.contains("min(720/iw\\,1280/ih)"), "{vf}");
    assert!(vf.contains("pad=720:1280:(ow-iw)/2:(oh-ih)/2"), "{vf}");
    assert!(vf.contains("setsar=1"), "{vf}");
    assert!(!vf.contains("force_original_aspect_ratio"), "{vf}");
  }

  #[test]
  fn majority_portrait_counts_then_ties_to_first() {
    assert!(majority_portrait(&[(1080, 1920), (1080, 1920), (1920, 1080)]));
    assert!(!majority_portrait(&[(1920, 1080), (1280, 720), (720, 1280)]));
    assert!(majority_portrait(&[(720, 1280), (1920, 1080)]));
    assert!(!majority_portrait(&[(1920, 1080), (720, 1280)]));
    assert!(majority_portrait(&[]));
  }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib output_spec -- --nocapture`

Expected: FAIL 找不到 `output_spec` 或其中函数

- [ ] **Step 3: Write minimal implementation**

`src-tauri/src/lib.rs` 在 `mod encode;` 后加 `mod output_spec;`。

`output_spec.rs`：

```rust
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionPreset {
  P720,
  P1080,
}

pub fn parse_resolution_preset(value: &str, fallback: ResolutionPreset) -> ResolutionPreset {
  match value.trim() {
    "720" => ResolutionPreset::P720,
    "1080" => ResolutionPreset::P1080,
    _ => fallback,
  }
}

pub fn target_canvas(preset: ResolutionPreset, width: u32, height: u32) -> (u32, u32) {
  let portrait = height > width;
  match (preset, portrait) {
    (ResolutionPreset::P720, true) => (720, 1280),
    (ResolutionPreset::P720, false) => (1280, 720),
    (ResolutionPreset::P1080, true) => (1080, 1920),
    (ResolutionPreset::P1080, false) => (1920, 1080),
  }
}

pub fn letterbox_vf(target_w: u32, target_h: u32) -> String {
  format!(
    "scale=w='trunc(iw*min({tw}/iw\\,{th}/ih)/2)*2':h='trunc(ih*min({tw}/iw\\,{th}/ih)/2)*2',pad={tw}:{th}:(ow-iw)/2:(oh-ih)/2:black,setsar=1",
    tw = target_w,
    th = target_h,
  )
}

pub fn majority_portrait(sizes: &[(u32, u32)]) -> bool {
  if sizes.is_empty() {
    return true;
  }
  let portrait = sizes.iter().filter(|(w, h)| *h > *w).count();
  let landscape = sizes.len() - portrait;
  if portrait > landscape {
    true
  } else if landscape > portrait {
    false
  } else {
    sizes[0].1 > sizes[0].0
  }
}
```

先保留 `use std::path::Path;`（Task 2 用）。若 Task 1 编译警告 unused，可先不加这行，Task 2 再加。

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib output_spec -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/output_spec.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
添加压制目标画幅与 letterbox 滤镜

EOF
)"
```

---

### Task 2: moov 位置与压制决策

**Files:**
- Modify: `src-tauri/src/output_spec.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub fn moov_is_before_mdat(path: &Path) -> bool`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum CompressAction { Skip, RemuxFaststart, Reencode }`
  - `pub fn decide_compress(h264: bool, src_w: u32, src_h: u32, target_w: u32, target_h: u32, moov_before_mdat: bool) -> CompressAction`

- [ ] **Step 1: Write the failing test**

在 `output_spec.rs` 的 `tests` 末尾追加（需要 `use std::fs; use std::io::Write;`）：

```rust
  fn write_boxes(path: &std::path::Path, boxes: &[(&[u8; 4], u32)]) {
    let mut f = fs::File::create(path).unwrap();
    for (kind, payload) in boxes {
      let size = 8 + payload;
      f.write_all(&size.to_be_bytes()).unwrap();
      f.write_all(*kind).unwrap();
      f.write_all(&vec![0u8; *payload as usize]).unwrap();
    }
  }

  #[test]
  fn moov_is_before_mdat_detects_faststart_and_postponed() {
    let dir = std::env::temp_dir().join(format!(
      "kuaiya-moov-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let front = dir.join("front.mp4");
    write_boxes(&front, &[(&*b"ftyp", 4), (&*b"moov", 16), (&*b"mdat", 32)]);
    let back = dir.join("back.mp4");
    write_boxes(&back, &[(&*b"ftyp", 4), (&*b"mdat", 32), (&*b"moov", 16)]);
    let empty = dir.join("empty.bin");
    fs::write(&empty, [0u8; 3]).unwrap();
    assert!(moov_is_before_mdat(&front));
    assert!(!moov_is_before_mdat(&back));
    assert!(!moov_is_before_mdat(&empty));
    fs::remove_dir_all(dir).unwrap();
  }

  #[test]
  fn decide_compress_skip_remux_reencode() {
    assert_eq!(
      decide_compress(true, 720, 1280, 720, 1280, true),
      CompressAction::Skip
    );
    assert_eq!(
      decide_compress(true, 720, 1280, 720, 1280, false),
      CompressAction::RemuxFaststart
    );
    assert_eq!(
      decide_compress(false, 720, 1280, 720, 1280, true),
      CompressAction::Reencode
    );
    assert_eq!(
      decide_compress(true, 1080, 1920, 720, 1280, true),
      CompressAction::Reencode
    );
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib decide_compress -- --nocapture`

Expected: FAIL `cannot find function decide_compress`

- [ ] **Step 3: Write minimal implementation**

```rust
use std::fs;
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressAction {
  Skip,
  RemuxFaststart,
  Reencode,
}

pub fn decide_compress(
  h264: bool,
  src_w: u32,
  src_h: u32,
  target_w: u32,
  target_h: u32,
  moov_before_mdat: bool,
) -> CompressAction {
  if h264 && src_w == target_w && src_h == target_h {
    if moov_before_mdat {
      CompressAction::Skip
    } else {
      CompressAction::RemuxFaststart
    }
  } else {
    CompressAction::Reencode
  }
}

pub fn moov_is_before_mdat(path: &Path) -> bool {
  let Ok(mut f) = fs::File::open(path) else {
    return false;
  };
  let Ok(meta) = f.metadata() else {
    return false;
  };
  let file_len = meta.len();
  let mut off = 0u64;
  let mut seen_moov: Option<u64> = None;
  let mut seen_mdat: Option<u64> = None;
  let mut hdr = [0u8; 8];
  for _ in 0..16 {
    if off + 8 > file_len {
      break;
    }
    if f.read_exact(&mut hdr).is_err() {
      break;
    }
    let size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
    let kind = &hdr[4..8];
    let box_size = if size == 1 {
      let mut wide = [0u8; 8];
      if f.read_exact(&mut wide).is_err() {
        break;
      }
      u64::from_be_bytes(wide)
    } else if size == 0 {
      file_len - off
    } else {
      size
    };
    if box_size < 8 {
      break;
    }
    if kind == b"moov" {
      seen_moov = Some(off);
    }
    if kind == b"mdat" {
      seen_mdat = Some(off);
    }
    if seen_moov.is_some() && seen_mdat.is_some() {
      break;
    }
    off = off.saturating_add(box_size);
    if std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(off)).is_err() {
      break;
    }
  }
  match (seen_moov, seen_mdat) {
    (Some(moov), Some(mdat)) => moov < mdat,
    _ => false,
  }
}
```

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib output_spec -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/output_spec.rs
git commit -m "$(cat <<'EOF'
添加 moov 前置探测与压制跳过决策

EOF
)"
```

---

### Task 3: 设置表增加两个分辨率字段

**Files:**
- Modify: `src-tauri/src/tasks.rs`

**Interfaces:**
- Consumes: `crate::output_spec::{parse_resolution_preset, ResolutionPreset}`
- Produces: `AppSettingsRecord` 增加 `compress_resolution: String`、`merge_resolution: String`（serde camelCase）

- [ ] **Step 1: Write the failing test**

在 `tasks.rs` 文件末尾：

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use rusqlite::Connection;

  #[test]
  fn migrate_adds_resolution_columns_with_defaults() {
    let conn = Connection::open_in_memory().unwrap();
    conn
      .execute_batch(
        r#"
        CREATE TABLE settings (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          concurrency INTEGER NOT NULL,
          scan_interval_minutes INTEGER NOT NULL,
          quality_preset TEXT NOT NULL DEFAULT 'size',
          auto_scan_on_launch INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO settings (id, concurrency, scan_interval_minutes, quality_preset, auto_scan_on_launch)
        VALUES (1, 2, 3, 'size', 0);
        "#,
      )
      .unwrap();
    migrate_settings_columns(&conn).unwrap();
    let cols = settings_columns(&conn).unwrap();
    assert!(cols.iter().any(|c| c == "compress_resolution"));
    assert!(cols.iter().any(|c| c == "merge_resolution"));
    let (c, m): (String, String) = conn
      .query_row(
        "SELECT compress_resolution, merge_resolution FROM settings WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
      )
      .unwrap();
    assert_eq!(c, "720");
    assert_eq!(m, "1080");
  }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib migrate_adds_resolution -- --nocapture`

Expected: FAIL（列不存在或 migrate 未加列）

- [ ] **Step 3: Write minimal implementation**

`AppSettingsRecord` 增加：

```rust
  pub compress_resolution: String,
  pub merge_resolution: String,
```

`migrate_settings_columns` 在现有列迁移后追加：

```rust
  let cols = settings_columns(conn)?;
  if !cols.iter().any(|c| c == "compress_resolution") {
    conn
      .execute(
        "ALTER TABLE settings ADD COLUMN compress_resolution TEXT NOT NULL DEFAULT '720'",
        [],
      )
      .map_err(|e| format!("升级设置表失败：{e}"))?;
  }
  let cols = settings_columns(conn)?;
  if !cols.iter().any(|c| c == "merge_resolution") {
    conn
      .execute(
        "ALTER TABLE settings ADD COLUMN merge_resolution TEXT NOT NULL DEFAULT '1080'",
        [],
      )
      .map_err(|e| format!("升级设置表失败：{e}"))?;
  }
```

`CREATE TABLE settings` 增加同样两列及默认值。

`INSERT OR IGNORE` 可不列这两列（靠表默认）。

`load_settings` SELECT 改为包含 `compress_resolution, merge_resolution`，映射：

```rust
use crate::output_spec::{parse_resolution_preset, ResolutionPreset};

fn normalize_compress_resolution(v: &str) -> String {
  match parse_resolution_preset(v, ResolutionPreset::P720) {
    ResolutionPreset::P720 => "720".into(),
    ResolutionPreset::P1080 => "1080".into(),
  }
}

fn normalize_merge_resolution(v: &str) -> String {
  match parse_resolution_preset(v, ResolutionPreset::P1080) {
    ResolutionPreset::P720 => "720".into(),
    ResolutionPreset::P1080 => "1080".into(),
  }
}
```

`save_settings` 的 INSERT/UPSERT 必须写入这两列，值先走上面两个 normalize。

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib migrate_adds_resolution -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/tasks.rs
git commit -m "$(cat <<'EOF'
设置增加压制与合成默认分辨率

EOF
)"
```

---

### Task 4: 系统设置 UI 与调用参数

**Files:**
- Modify: `src/types.ts`
- Modify: `src/settings.ts`
- Modify: `src/views/SettingsView.vue`
- Modify: `src/compress.ts`
- Modify: `src/merge.ts`
- Modify: `src/App.vue`
- Create: `src/settings.test.ts`

**Interfaces:**
- Consumes: 后端 `compressResolution` / `mergeResolution`
- Produces:
  - `export type ResolutionPreset = '720' | '1080'`
  - `AppSettings.compressResolution` / `mergeResolution`
  - `getCompressResolution(): ResolutionPreset`
  - `getMergeResolution(): ResolutionPreset`
  - `compressVideo` invoke 增加 `targetResolution`
  - `mergeVideos` invoke 增加 `mergeResolution`

- [ ] **Step 1: Write the failing test**

`src/settings.test.ts`：

```ts
/// <reference types="node" />

import assert from 'node:assert/strict'
import test from 'node:test'
import { normalizeResolutionPreset } from './settings.ts'

test('compress fallback is 720, merge fallback is 1080', () => {
  assert.equal(normalizeResolutionPreset('720', '720'), '720')
  assert.equal(normalizeResolutionPreset('1080', '720'), '1080')
  assert.equal(normalizeResolutionPreset('4k', '720'), '720')
  assert.equal(normalizeResolutionPreset('720p', '1080'), '1080')
  assert.equal(normalizeResolutionPreset('', '1080'), '1080')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node --experimental-strip-types --test src/settings.test.ts`

Expected: FAIL 无法导出 `normalizeResolutionPreset`

- [ ] **Step 3: Write minimal implementation**

`src/types.ts`：

```ts
export type ResolutionPreset = '720' | '1080'

export type AppSettings = {
  concurrency: number
  scanIntervalMinutes: number
  qualityPreset: QualityPreset
  autoScanOnLaunch: boolean
  compressResolution: ResolutionPreset
  mergeResolution: ResolutionPreset
}
```

`src/settings.ts`：导出

```ts
export function normalizeResolutionPreset(
  value: unknown,
  fallback: ResolutionPreset,
): ResolutionPreset {
  return value === '720' || value === '1080' ? value : fallback
}
```

`defaults`：`compressResolution: '720'`，`mergeResolution: '1080'`。

`normalize()` / `updateSettings()` / `persist()` 带上这两项。`persist` 的 invoke 对象增加 `compressResolution`、`mergeResolution`。

```ts
export function getCompressResolution(): ResolutionPreset {
  return settings.value.compressResolution
}

export function getMergeResolution(): ResolutionPreset {
  return settings.value.mergeResolution
}

export function resolutionPresetLabel(preset: ResolutionPreset): string {
  return preset === '720' ? '720p' : '1080p'
}
```

`SettingsView.vue`：两个与画质档位相同的 `preset-switch`（720p / 1080p）。`isDirty`、`syncFromSaved`、`saveSettings` 包含这两项。画质档位说明「分辨率均不变」改为「画面缩放到下面的默认分辨率（不足则放大，比例不对补黑边）」。把「启动时自动扫描」行的 `last` 挪到最后一个新行。

`src/compress.ts` invoke 增加 `targetResolution: getCompressResolution()`。

`src/merge.ts` invoke 增加 `mergeResolution: getMergeResolution()`。

`src/App.vue` 侧栏：`{{ resolutionPresetLabel(settings.compressResolution) }} · {{ qualityPresetLabel(settings.qualityPreset) }}`。

- [ ] **Step 4: Run tests and make sure they pass**

Run: `node --experimental-strip-types --test src/settings.test.ts`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/settings.ts src/settings.test.ts src/views/SettingsView.vue src/compress.ts src/merge.ts src/App.vue
git commit -m "$(cat <<'EOF'
系统设置增加压制与合成分辨率选项

EOF
)"
```

---

### Task 5: 接入压制跳过 / 重排 / 重编码

**Files:**
- Modify: `src-tauri/src/compress.rs`

**Interfaces:**
- Consumes: `output_spec::{decide_compress, letterbox_vf, moov_is_before_mdat, parse_resolution_preset, target_canvas, CompressAction, ResolutionPreset}`
- Produces: `compress_video(..., target_resolution: Option<String>)`；跳过 encoder 短标签 `"copy"`；重排 `"copy"`；重编码仍为 x264/NVENC 等

- [ ] **Step 1: Write the failing test**

在 `compress.rs` 的 `tests` 里、现有 `fake_ffprobe` 测试旁追加：

```rust
  #[cfg(unix)]
  #[test]
  fn probe_video_codec_reads_h264_name() {
    let (dir, ffprobe) = fake_ffprobe("h264", true);
    let input = dir.join("clip.mp4");
    fs::write(&input, MINIMAL_Y4M_VIDEO).unwrap();
    assert_eq!(probe_video_codec(&ffprobe, &input).unwrap(), "h264");
    fs::remove_dir_all(dir).unwrap();
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib probe_video_codec_reads_h264 -- --nocapture`

Expected: FAIL `cannot find function probe_video_codec`

- [ ] **Step 3: Write minimal implementation**

`compress_video` 增加参数 `target_resolution: Option<String>`（放在 `quality_preset` 旁）。

探测完 `input_wh` 后：

```rust
    use crate::output_spec::{
      decide_compress, letterbox_vf, moov_is_before_mdat, parse_resolution_preset, target_canvas,
      CompressAction, ResolutionPreset,
    };

    let preset_res = parse_resolution_preset(
      target_resolution.as_deref().unwrap_or("720"),
      ResolutionPreset::P720,
    );
    let target_wh = target_canvas(preset_res, input_wh.0, input_wh.1);
    let codec = probe_video_codec(&ffprobe, &input)?;
    let h264 = codec.eq_ignore_ascii_case("h264");
    let moov_front = moov_is_before_mdat(&input);
    let action = decide_compress(h264, input_wh.0, input_wh.1, target_wh.0, target_wh.1, moov_front);
```

新增 `probe_video_codec`：与 `probe_video_size` 相同 subprocess，`-show_entries stream=codec_name`，`-of default=noprint_wrappers=1:nokey=1`，trim 后返回。

`spawn_blocking` 按 `action` 分支（把 `action`、`target_wh`、`input`、路径、`ffmpeg` 等 move 进去）：

**Skip：**
- 若 `inplace`：`fs::metadata(&input)` 得到 size，emit progress 100，返回 `CompressResult { output_path: input 字符串, output_size, encoder: "copy".into() }`
- 否则：`fs::copy(&input, &final_path_clone)`，校验 `assert_resolution(..., target_wh)`（源已是目标画幅），同样返回 `"copy"`

**RemuxFaststart：** 构建：

```
ffmpeg -y -hide_banner -loglevel error -i INPUT
  -map 0:v:0 -map 0:a:0?
  -c copy -movflags +faststart
  -progress pipe:1 -nostats
  ENCODE_PATH
```

沿用现有 stdout 进度 / stderr 收集 / `cancel_key` / `child.kill`。失败：删临时文件，**不要 return**，落入重编码（同一 `target_wh`）。成功：`assert_resolution(..., target_wh)`，inplace 则 `replace_file_force`，encoder `"copy"`。

**Reencode：** 现有 encoder 循环，但：

- `cmd.args` 在 `-i` 之后加 `-map 0:v:0 -map 0:a:0?`
- 加 `-vf` `{letterbox_vf(target_wh.0, target_wh.1)}`
- 保留 `append_video_encode_args` / `append_audio_aac_args` / `+faststart`
- `assert_resolution(..., target_wh)` 不再用 `input_wh`

硬编回退链不变。

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --nocapture`

Expected: PASS 全部 lib 测试

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/compress.rs
git commit -m "$(cat <<'EOF'
压制按需跳过、重排 moov 或转 H.264

EOF
)"
```

---

### Task 6: 合成不一致时用设置画幅 letterbox

**Files:**
- Modify: `src-tauri/src/media.rs`

**Interfaces:**
- Consumes: `target_canvas`、`parse_resolution_preset`、`majority_portrait`、`letterbox_vf`、`ResolutionPreset`
- Produces: `merge_videos(..., merge_resolution: Option<String>)`；分辨率不一致时 `target_wh` 为合成档位画幅；`build_normalize_filter` 视频链改为 letterbox 而不是拉伸 `scale=W:H`

- [ ] **Step 1: Write the failing test**

在 `media.rs` tests 追加：

```rust
  #[test]
  fn merge_mismatch_target_uses_settings_not_first_clip() {
    use crate::output_spec::{majority_portrait, parse_resolution_preset, target_canvas, ResolutionPreset};
    let sizes = [(1920, 1080), (1080, 1920), (1080, 1920)];
    let portrait = majority_portrait(&sizes);
    assert!(portrait);
    let preset = parse_resolution_preset("1080", ResolutionPreset::P1080);
    let dummy = if portrait { (9, 16) } else { (16, 9) };
    assert_eq!(target_canvas(preset, dummy.0, dummy.1), (1080, 1920));
  }

  #[test]
  fn normalize_filter_letterboxes_instead_of_stretch_scale() {
    let (filter, _) =
      build_normalize_filter(2, 720, 1280, &[true, true], &[1.0, 1.0], "25/1");
    assert!(filter.contains("pad=720:1280:(ow-iw)/2:(oh-ih)/2"), "{filter}");
    assert!(!filter.contains("scale=720:1280:flags=bicubic"), "{filter}");
  }
```

把现有若断言 `scale={width}:{height}:flags=bicubic` 的测试改成断言 pad/letterbox（不要留拉伸断言）。

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib normalize_filter_letterboxes -- --nocapture`

Expected: FAIL 滤镜仍是拉伸 scale

- [ ] **Step 3: Write minimal implementation**

`merge_videos` 增加 `merge_resolution: Option<String>`。

替换：

```rust
  let first_wh = sizes[0];
  let mismatched = sizes.iter().any(|wh| *wh != first_wh);
  if mismatched && !normalize {
    return Err(format!(
      "所选视频分辨率不同，无法直接合成（首个为 {}×{}）。请统一分辨率后再试",
      first_wh.0, first_wh.1
    ));
  }

  let target_wh = (even_dim(first_wh.0), even_dim(first_wh.1));
  let assert_wh = if mismatched { target_wh } else { first_wh };
```

为：

```rust
  use crate::output_spec::{
    letterbox_vf, majority_portrait, parse_resolution_preset, target_canvas, ResolutionPreset,
  };

  let first_wh = sizes[0];
  let mismatched = sizes.iter().any(|wh| *wh != first_wh);
  if mismatched && !normalize {
    return Err(format!(
      "所选视频分辨率不同，无法直接合成（首个为 {}×{}）。请统一分辨率后再试",
      first_wh.0, first_wh.1
    ));
  }

  let merge_preset = parse_resolution_preset(
    merge_resolution.as_deref().unwrap_or("1080"),
    ResolutionPreset::P1080,
  );
  let (target_w, target_h) = if mismatched {
    if majority_portrait(&sizes) {
      target_canvas(merge_preset, 9, 16)
    } else {
      target_canvas(merge_preset, 16, 9)
    }
  } else {
    first_wh
  };
  let target_wh = (even_dim(target_w), even_dim(target_h));
  let assert_wh = if mismatched { target_wh } else { first_wh };
```

`build_normalize_filter` 里视频滤镜一行改为：

```rust
    let vf = letterbox_vf(width, height);
    parts.push(format!(
      "[{i}:v]{vf},format=yuv420p,setpts=PTS-STARTPTS,fps={fps}[v{i}]"
    ));
```

不要去掉现有 `+faststart`。规范化单片路径仍不 `scale`（`normalize_clip_vf` 不变）。

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --nocapture`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
合成分辨率不一致时对齐到设置画幅

EOF
)"
```

---
