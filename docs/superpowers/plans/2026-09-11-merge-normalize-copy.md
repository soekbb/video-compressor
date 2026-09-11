# 合成只转不匹配分集 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 分辨率一致但少数分集帧率/timebase 不同时，只重编码这些分集，再 concat demuxer `-c copy`。

**Architecture:** 在 `media.rs` 用视频指纹多数派选目标；对不一致分集 x264 规范化到临时文件并探测校验，再走现有 copy 拼接。失败回退 `FullReencode`。不改前端。

**Tech Stack:** Rust / FFmpeg（libx264）/ 现有 `StreamFingerprint` 与 `cargo test --lib`

## Global Constraints

- 分辨率不一致：未勾选统一分辨率则报错；勾选了直接 `FullReencode`（不走本路径）
- 规范化只用 libx264（不用硬编）
- 分辨率已与目标相同则不 `scale`
- 成功/失败/取消都删除 `kuaiya-norm-{任务id}-{index}.mp4`
- 取消仍用现有 `cancel_key`，尽快停当前 FFmpeg
- 此路径失败必须回退 `FullReencode`，不能直接报失败（除非用户取消）

**Spec:** `docs/superpowers/specs/2026-09-11-merge-normalize-copy-design.md`

---

### Task 1: 多数派目标与待规范化下标

**Files:**
- Modify: `src-tauri/src/media.rs`（`can_copy_video` 附近新增函数；`mod tests`）

**Interfaces:**
- Consumes: `StreamFingerprint`、`can_copy_video`
- Produces:
  - `fn majority_video_target_index(fingerprints: &[StreamFingerprint]) -> usize`
  - `fn indices_needing_video_normalize(fingerprints: &[StreamFingerprint]) -> Vec<usize>`

- [ ] **Step 1: Write the failing test**

在 `media.rs` 的 `tests` 模块、现有 `fingerprint` helper 之后新增：

```rust
  #[test]
  fn majority_target_ignores_leading_minority_fps() {
    let fps24 = fingerprint("24/1", "1/12288");
    let fps25 = fingerprint("25/1", "1/12800");
    let list = vec![
      fps24.clone(),
      fps25.clone(),
      fps25.clone(),
      fps25.clone(),
      fps25.clone(),
    ];
    assert_eq!(majority_video_target_index(&list), 1);
    assert_eq!(indices_needing_video_normalize(&list), vec![0]);
  }

  #[test]
  fn majority_target_tie_uses_lower_index() {
    let a = fingerprint("25/1", "1/12800");
    let b = fingerprint("24/1", "1/12288");
    let list = vec![a, b];
    assert_eq!(majority_video_target_index(&list), 0);
    assert_eq!(indices_needing_video_normalize(&list), vec![1]);
  }

  #[test]
  fn majority_target_all_matching_needs_no_normalize() {
    let a = fingerprint("25/1", "1/12800");
    let list = vec![a.clone(), a.clone(), a];
    assert_eq!(majority_video_target_index(&list), 0);
    assert!(indices_needing_video_normalize(&list).is_empty());
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib majority_target -- --nocapture`

Expected: FAIL 编译错误 `cannot find function majority_video_target_index`

- [ ] **Step 3: Write minimal implementation**

放在 `can_copy_video` 之后：

```rust
fn video_compat_tuple(
  fp: &StreamFingerprint,
) -> (&u32, &u32, &str, &str, &str, &str, &str) {
  (
    &fp.width,
    &fp.height,
    fp.video_codec.as_str(),
    fp.pix_fmt.as_str(),
    fp.video_profile.as_str(),
    fp.r_frame_rate.as_str(),
    fp.time_base.as_str(),
  )
}

fn majority_video_target_index(fingerprints: &[StreamFingerprint]) -> usize {
  let mut best_index = 0;
  let mut best_count = 0;
  for (i, fp) in fingerprints.iter().enumerate() {
    let count = fingerprints
      .iter()
      .filter(|other| video_compat_tuple(other) == video_compat_tuple(fp))
      .count();
    if count > best_count {
      best_count = count;
      best_index = i;
    }
  }
  best_index
}

fn indices_needing_video_normalize(fingerprints: &[StreamFingerprint]) -> Vec<usize> {
  if fingerprints.is_empty() {
    return Vec::new();
  }
  let target = &fingerprints[majority_video_target_index(fingerprints)];
  fingerprints
    .iter()
    .enumerate()
    .filter(|(_, fp)| {
      !can_copy_video(&[target.clone(), (*fp).clone()])
    })
    .map(|(i, _)| i)
    .collect()
}
```

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib majority_target`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
合成按多数派指纹选出需规范化的分集

EOF
)"
```

---

### Task 2: 解析 MP4 timescale

**Files:**
- Modify: `src-tauri/src/media.rs`

**Interfaces:**
- Consumes: 目标 `time_base` 字符串
- Produces: `fn mp4_timescale(time_base: &str) -> Option<u32>`

- [ ] **Step 1: Write the failing test**

```rust
  #[test]
  fn mp4_timescale_parses_one_over_n() {
    assert_eq!(mp4_timescale("1/12800"), Some(12800));
    assert_eq!(mp4_timescale("1/12288"), Some(12288));
    assert_eq!(mp4_timescale("1/12800"), Some(12800));
  }

  #[test]
  fn mp4_timescale_rejects_unusable_values() {
    assert_eq!(mp4_timescale(""), None);
    assert_eq!(mp4_timescale("1/0"), None);
    assert_eq!(mp4_timescale("2/12800"), None);
    assert_eq!(mp4_timescale("12800"), None);
    assert_eq!(mp4_timescale("n/a"), None);
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib mp4_timescale`

Expected: FAIL `cannot find function mp4_timescale`

- [ ] **Step 3: Write minimal implementation**

```rust
fn mp4_timescale(time_base: &str) -> Option<u32> {
  let (num, den) = time_base.trim().split_once('/')?;
  if num != "1" {
    return None;
  }
  let n: u32 = den.parse().ok()?;
  if n == 0 {
    None
  } else {
    Some(n)
  }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib mp4_timescale`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
解析合成目标 MP4 timescale

EOF
)"
```

---

### Task 3: 策略枚举与选择顺序

**Files:**
- Modify: `src-tauri/src/media.rs`（`ConcatStrategy`、`uses_filter_concat`、抽出 `concat_strategies`、`merge_videos` 里组装 strategies 的代码改为调用它）

**Interfaces:**
- Consumes: `ConcatStrategy` 现有三变体
- Produces:
  - `ConcatStrategy::NormalizeThenCopy`
  - `fn concat_strategies(resolution_mismatched: bool, full_copy_ok: bool, video_copy_ok: bool, all_have_audio: bool, needs_video_normalize: bool) -> Vec<ConcatStrategy>`
  - `uses_filter_concat` 对 `NormalizeThenCopy` 为 false

- [ ] **Step 1: Write the failing test**

```rust
  #[test]
  fn concat_strategies_inserts_normalize_before_full_reencode() {
    let got = concat_strategies(false, false, false, true, true);
    assert_eq!(
      got,
      vec![
        ConcatStrategy::NormalizeThenCopy,
        ConcatStrategy::FullReencode
      ]
    );
  }

  #[test]
  fn concat_strategies_skips_normalize_when_copy_ok() {
    let got = concat_strategies(false, true, true, true, false);
    assert_eq!(
      got,
      vec![
        ConcatStrategy::FullCopy,
        ConcatStrategy::VideoCopyAudioEncode,
        ConcatStrategy::FullReencode
      ]
    );
  }

  #[test]
  fn concat_strategies_resolution_mismatch_only_reencodes() {
    let got = concat_strategies(true, false, false, true, true);
    assert_eq!(got, vec![ConcatStrategy::FullReencode]);
  }

  #[test]
  fn filter_concat_not_used_for_normalize_then_copy() {
    assert!(!uses_filter_concat(false, ConcatStrategy::NormalizeThenCopy));
    assert!(uses_filter_concat(false, ConcatStrategy::FullReencode));
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib concat_strategies`

Expected: FAIL（无 `NormalizeThenCopy` 或 `concat_strategies`）

- [ ] **Step 3: Write minimal implementation**

`ConcatStrategy` 增加：

```rust
  /// 只转与多数派不一致的分集，再 demuxer copy
  NormalizeThenCopy,
```

```rust
fn uses_filter_concat(mismatched: bool, strategy: ConcatStrategy) -> bool {
  mismatched || strategy == ConcatStrategy::FullReencode
}

fn concat_strategies(
  resolution_mismatched: bool,
  full_copy_ok: bool,
  video_copy_ok: bool,
  all_have_audio: bool,
  needs_video_normalize: bool,
) -> Vec<ConcatStrategy> {
  if resolution_mismatched {
    return vec![ConcatStrategy::FullReencode];
  }
  let mut strategies = Vec::new();
  if full_copy_ok {
    strategies.push(ConcatStrategy::FullCopy);
  }
  if all_have_audio && video_copy_ok {
    strategies.push(ConcatStrategy::VideoCopyAudioEncode);
  }
  if needs_video_normalize {
    strategies.push(ConcatStrategy::NormalizeThenCopy);
  }
  strategies.push(ConcatStrategy::FullReencode);
  strategies
}
```

`merge_videos` 里替换手写 `strategies` 向量：

```rust
    let needs_video_normalize = !mismatched && !indices_needing_video_normalize(&fingerprints).is_empty();
    let mut strategies = concat_strategies(
      mismatched,
      full_copy_ok,
      video_copy_ok,
      all_have_audio,
      needs_video_normalize,
    );
```

`run_once` 的 `match strategy` 里为 `NormalizeThenCopy` 暂先 `return Err("内部错误：NormalizeThenCopy 未实现".into());`（下一任务接上）。所有 `match ConcatStrategy` 必须补上这一臂，否则无法编译。

`encoder_fallback_chain` 匹配处：`NormalizeThenCopy` 与 copy 类一样用占位 `vec![VideoEncoderKind::X264]`（真正编码在规范化子进程里用 x264，不走硬编链）。

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: PASS（含既有 22+ 新测试）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
合成在全量重编码前插入只转不匹配分集策略

EOF
)"
```

---

### Task 4: 单片规范化滤镜与临时路径

**Files:**
- Modify: `src-tauri/src/media.rs`

**Interfaces:**
- Consumes: `fps_filter_arg`、`mp4_timescale`
- Produces:
  - `fn normalize_clip_vf(target_fps: &str) -> String` → `setpts=PTS-STARTPTS,fps=25/1`（无 scale）
  - `fn normalize_temp_path(task_id: &str, index: usize) -> PathBuf`

- [ ] **Step 1: Write the failing test**

```rust
  #[test]
  fn normalize_clip_vf_resets_pts_and_fps_without_scale() {
    let vf = normalize_clip_vf("25/1");
    assert_eq!(vf, "setpts=PTS-STARTPTS,fps=25/1");
    assert!(!vf.contains("scale="));
  }

  #[test]
  fn normalize_temp_path_includes_task_id_and_index() {
    let p = normalize_temp_path("abc", 3);
    let name = p.file_name().unwrap().to_string_lossy();
    assert_eq!(name, "kuaiya-norm-abc-3.mp4");
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib normalize_clip_vf`

Expected: FAIL `cannot find function normalize_clip_vf`

- [ ] **Step 3: Write minimal implementation**

```rust
fn normalize_clip_vf(target_fps: &str) -> String {
  format!("setpts=PTS-STARTPTS,fps={}", fps_filter_arg(target_fps))
}

fn normalize_temp_path(task_id: &str, index: usize) -> PathBuf {
  std::env::temp_dir().join(format!("kuaiya-norm-{task_id}-{index}.mp4"))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib normalize_clip`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
添加分集规范化滤镜与临时文件路径

EOF
)"
```

---

### Task 5: 执行 NormalizeThenCopy 并回退

**Files:**
- Modify: `src-tauri/src/media.rs`（`merge_videos` 的 `run_once` / 策略循环）

**Interfaces:**
- Consumes: Task 1–4 函数、`probe_stream_fingerprint`、`append_video_encode_args`、`can_copy_video`、`escape_concat_path`、现有 cancel/progress
- Produces: `NormalizeThenCopy` 真正跑通：规范化 → 探测 → concat copy；失败回退 FullReencode；清理临时文件

- [ ] **Step 1: Write the failing test for progress mapping**

```rust
  #[test]
  fn normalize_phase_progress_maps_duration_to_90() {
    assert_eq!(normalize_phase_progress(0.0, 10.0), 0);
    assert_eq!(normalize_phase_progress(5.0, 10.0), 45);
    assert_eq!(normalize_phase_progress(10.0, 10.0), 90);
    assert_eq!(normalize_phase_progress(12.0, 10.0), 90);
  }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib normalize_phase_progress`

Expected: FAIL `cannot find function normalize_phase_progress`

- [ ] **Step 3: Implement progress helper + NormalizeThenCopy 执行**

```rust
fn normalize_phase_progress(done_secs: f64, total_secs: f64) -> u32 {
  if total_secs <= 0.0 {
    return 90;
  }
  let ratio = (done_secs / total_secs).clamp(0.0, 1.0);
  (ratio * 90.0).floor() as u32
}
```

在 `run_once` 中，`uses_filter_concat` 为 false 的分支里，将 `NormalizeThenCopy` 从占位错误改为：

1. `let target_i = majority_video_target_index(&fingerprints);`
2. `let target = fingerprints[target_i].clone();`
3. `let timescale = mp4_timescale(&target.time_base).ok_or(...)?;` 失败则 return Err 让外层回退
4. `let need = indices_needing_video_normalize(&fingerprints);`
5. `let mut concat_paths = input_paths_clone.clone();`
6. `let mut temps: Vec<PathBuf> = Vec::new();`
7. 对每个 `i in need`：
   - 若 `cancel.is_cancelled(&cancel_key)`：删 temps，return 已取消
   - 临时路径 `normalize_temp_path(&id, i)`
   - `Command::new(ffmpeg)`：`-y -hide_banner -loglevel error -i {input}` `-map 0:v:0`，若该片 `has_audio` 则 `-map 0:a:0?`
   - `-vf` `normalize_clip_vf(&target.r_frame_rate)`
   - `append_video_encode_args(..., X264, preset, target.width, target.height)` 再补 `-bf` `0` 与 `-video_track_timescale` `{timescale}`
   - 音频：若 `fp` 与 target 的 `audio_codec/sample_rate/channels/has_audio` 全等则 `-c:a copy`，否则 `append_audio_aac_unified_args`
   - `-movflags +faststart` 输出临时文件
   - 用与现有合成相同的 stdout progress：`out_time` 累加到 `done_secs`（本片之前已完成时长 + 本片 out_time），`emit` `normalize_phase_progress`
   - wait 失败：删 temps 与本片输出，return Err
   - `probe_stream_fingerprint`，`can_copy_video(&[target.clone(), probed])` 为 false 则 return Err
   - `concat_paths[i] = temp; temps.push(temp)`
8. 写 concat list（`escape_concat_path`），`-f concat -safe 0 -i list`
9. 若规范化后路径对应的指纹音频全一致（用 target + 未转分集原指纹；已转分集用 probed）：`-c copy`；否则 `-c:v copy` + `append_audio_aac_unified_args`
10. 成功后删 temps；失败/取消同样删（`defer` 用 `let _ = fs::remove_file` 循环）
11. copy 阶段进度从 90 升到 99（沿用 `parse_out_time_secs`，copy 很快）

规范化 FFmpeg 必须 `configure_subprocess`。不要在这一路径用硬编。

`run_once` 目前只接收 `strategy`+`encoder`，需要能读到 `fingerprints`。`fingerprints` 在 `spawn_blocking` 之前已存在，**move 进 closure**（若尚未 move：在 `spawn_blocking(move ||` 前不要再使用到需要 move 的值；`needs_video_normalize` 已在外面算过，把 `fingerprints` clone/`move` 进 blocking 闭包）。

策略循环：`NormalizeThenCopy` 失败且非取消时 `reset_progress()` 后 `break` 该 strategy（与 copy 类相同，不轮询硬编），然后进入 `FullReencode`。

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: PASS，无 warning（所有 `ConcatStrategy` match 穷尽）

手工（有 `tmp/她替三代人讨债-法语分集` 时可选）：用应用合成 24+25，确认输出视频/音频时长差 < 0.2s，且耗时远小于全量重编码。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/media.rs
git commit -m "$(cat <<'EOF'
合成只转不匹配分集后再无损拼接

EOF
)"
```
