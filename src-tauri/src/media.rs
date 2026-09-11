use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::compress::{
  assert_resolution, configure_subprocess, probe_video_size, spawn_stderr_collector, CompressState,
};
use crate::encode::{
  append_audio_aac_args, append_audio_aac_unified_args, append_video_encode_args,
  encoder_fallback_chain, mark_hw_encoder_failed, VideoEncoderKind,
};

const VIDEO_EXTS: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "flv"];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DramaVideo {
  pub name: String,
  pub path: String,
  pub size: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DramaFolder {
  pub name: String,
  pub path: String,
  pub video_count: usize,
  pub created_at_ms: u64,
  pub videos: Vec<DramaVideo>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DramaScanProgress {
  pub dirs_scanned: u32,
  pub dramas_found: u32,
  pub videos_found: u32,
  pub current_name: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeProgressPayload {
  pub id: String,
  pub progress: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
  pub output_path: String,
  pub output_size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDimensions {
  pub width: u32,
  pub height: u32,
}

fn even_dim(n: u32) -> u32 {
  let v = (n / 2) * 2;
  if v == 0 {
    2
  } else {
    v
  }
}

/// 用于判断 concat 是否可 stream copy（参数不一致则必须重编码）。
/// 帧率/time_base 必须计入：copy 拼接时 timescale 不同会导致画面卡住、声音继续。
#[derive(Clone, Debug, PartialEq, Eq)]
struct StreamFingerprint {
  width: u32,
  height: u32,
  video_codec: String,
  pix_fmt: String,
  video_profile: String,
  r_frame_rate: String,
  time_base: String,
  has_audio: bool,
  audio_codec: String,
  sample_rate: String,
  channels: String,
}

/// ffprobe csv 字段顺序不稳定（常把 profile 插到 width 前），改用 key=value。
fn ffprobe_entries(
  ffprobe: &Path,
  input: &Path,
  select: &str,
  entries: &str,
) -> Option<std::collections::HashMap<String, String>> {
  let mut command = Command::new(ffprobe);
  configure_subprocess(&mut command);
  let output = command
    .args([
      "-v",
      "error",
      "-select_streams",
      select,
      "-show_entries",
      entries,
      "-of",
      "default=noprint_wrappers=1:nokey=0",
      input.to_string_lossy().as_ref(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let mut map = std::collections::HashMap::new();
  for line in String::from_utf8_lossy(&output.stdout).lines() {
    let line = line.trim();
    if line.is_empty() {
      continue;
    }
    if let Some((k, v)) = line.split_once('=') {
      map.insert(k.trim().to_string(), v.trim().to_string());
    }
  }
  if map.is_empty() {
    None
  } else {
    Some(map)
  }
}

fn map_get_lc(map: &std::collections::HashMap<String, String>, key: &str) -> String {
  map
    .get(key)
    .map(|v| v.trim().to_ascii_lowercase())
    .filter(|v| !v.is_empty() && v != "n/a" && v != "unknown")
    .unwrap_or_default()
}

fn probe_stream_fingerprint(ffprobe: &Path, input: &Path) -> Result<StreamFingerprint, String> {
  let video = ffprobe_entries(
    ffprobe,
    input,
    "v:0",
    "stream=codec_name,width,height,pix_fmt,profile,r_frame_rate,time_base",
  )
  .ok_or_else(|| format!("无法读取视频流：{}", input.display()))?;

  let video_codec = map_get_lc(&video, "codec_name");
  let width: u32 = video
    .get("width")
    .and_then(|v| v.trim().parse().ok())
    .ok_or_else(|| format!("无法读取宽度：{}", input.display()))?;
  let height: u32 = video
    .get("height")
    .and_then(|v| v.trim().parse().ok())
    .ok_or_else(|| format!("无法读取高度：{}", input.display()))?;
  let pix_fmt = map_get_lc(&video, "pix_fmt");
  let video_profile = map_get_lc(&video, "profile");
  let r_frame_rate = map_get_lc(&video, "r_frame_rate");
  let time_base = map_get_lc(&video, "time_base");

  if video_codec.is_empty() || width == 0 || height == 0 {
    return Err(format!("视频流无效：{}", input.display()));
  }

  let audio = ffprobe_entries(
    ffprobe,
    input,
    "a:0",
    "stream=codec_name,sample_rate,channels",
  );
  let (has_audio, audio_codec, sample_rate, channels) = match audio {
    Some(map) => {
      let codec = map_get_lc(&map, "codec_name");
      if codec.is_empty() {
        (false, String::new(), String::new(), String::new())
      } else {
        (
          true,
          codec,
          map.get("sample_rate").cloned().unwrap_or_default(),
          map.get("channels").cloned().unwrap_or_default(),
        )
      }
    }
    None => (false, String::new(), String::new(), String::new()),
  };

  Ok(StreamFingerprint {
    width,
    height,
    video_codec,
    pix_fmt,
    video_profile,
    r_frame_rate,
    time_base,
    has_audio,
    audio_codec,
    sample_rate,
    channels,
  })
}

fn can_stream_copy(fingerprints: &[StreamFingerprint]) -> bool {
  let Some(first) = fingerprints.first() else {
    return false;
  };
  // 无音频轨时也可 copy；但各片是否有音频、音频参数必须完全一致
  fingerprints.iter().all(|fp| fp == first)
}

fn can_copy_video(fingerprints: &[StreamFingerprint]) -> bool {
  let Some(first) = fingerprints.first() else {
    return false;
  };
  fingerprints.iter().all(|fp| {
    fp.width == first.width
      && fp.height == first.height
      && fp.video_codec == first.video_codec
      && fp.pix_fmt == first.pix_fmt
      && fp.video_profile == first.video_profile
      && !fp.r_frame_rate.is_empty()
      && fp.r_frame_rate == first.r_frame_rate
      && !fp.time_base.is_empty()
      && fp.time_base == first.time_base
  })
}

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
    .filter(|(_, fp)| !can_copy_video(&[target.clone(), (*fp).clone()]))
    .map(|(i, _)| i)
    .collect()
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConcatStrategy {
  /// 音视频全部 copy
  FullCopy,
  /// 视频 copy，音频统一重编码为 AAC 48k 立体声
  VideoCopyAudioEncode,
  /// 全量重编码（硬编优先）
  FullReencode,
  /// 只转与多数派不一致的分集，再 demuxer copy
  NormalizeThenCopy,
}

/// concat demuxer 在 timebase/帧率不一致时会压扁视频时间戳；重编码也必须走 filter concat。
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

fn fps_filter_arg(r_frame_rate: &str) -> String {
  let s = r_frame_rate.trim();
  if !s.is_empty()
    && s.chars().all(|c| c.is_ascii_digit() || c == '/')
    && s.chars().any(|c| c.is_ascii_digit())
    && !s.starts_with('/')
    && !s.ends_with('/')
  {
    s.to_string()
  } else {
    "25".to_string()
  }
}

fn normalize_clip_vf(target_fps: &str) -> String {
  format!("setpts=PTS-STARTPTS,fps={}", fps_filter_arg(target_fps))
}

fn normalize_temp_path(task_id: &str, index: usize) -> PathBuf {
  std::env::temp_dir().join(format!("kuaiya-norm-{task_id}-{index}.mp4"))
}

/// 将各片段统一分辨率/帧率并重置时间戳后 concat。
fn build_normalize_filter(
  count: usize,
  width: u32,
  height: u32,
  has_audio: &[bool],
  durations: &[f64],
  fps: &str,
) -> (String, bool) {
  let any_audio = has_audio.iter().any(|v| *v);
  let mut parts: Vec<String> = Vec::new();
  let fps = fps_filter_arg(fps);

  for i in 0..count {
    parts.push(format!(
      "[{i}:v]scale={width}:{height}:flags=bicubic,setsar=1,format=yuv420p,setpts=PTS-STARTPTS,fps={fps}[v{i}]"
    ));
    if any_audio {
      if has_audio.get(i).copied().unwrap_or(false) {
        parts.push(format!(
          "[{i}:a]aformat=sample_fmts=fltp:sample_rates=44100:channel_layouts=stereo,aresample=async=1:first_pts=0,asetpts=PTS-STARTPTS[a{i}]"
        ));
      } else {
        let dur = durations.get(i).copied().unwrap_or(1.0).max(0.1);
        parts.push(format!(
          "anullsrc=channel_layout=stereo:sample_rate=44100,atrim=0:{dur},asetpts=PTS-STARTPTS[a{i}]"
        ));
      }
    }
  }

  let mut concat_in = String::new();
  for i in 0..count {
    concat_in.push_str(&format!("[v{i}]"));
    if any_audio {
      concat_in.push_str(&format!("[a{i}]"));
    }
  }
  if any_audio {
    parts.push(format!(
      "{concat_in}concat=n={count}:v=1:a=1[outv][outa]"
    ));
  } else {
    parts.push(format!("{concat_in}concat=n={count}:v=1:a=0[outv]"));
  }

  (parts.join(";"), any_audio)
}

#[tauri::command]
pub fn probe_video_dimensions(app: AppHandle, path: String) -> Result<VideoDimensions, String> {
  let ffprobe = resolve_bin(&app, "ffprobe")?;
  let input = PathBuf::from(&path);
  if !input.is_file() {
    return Err(format!("找不到文件：{path}"));
  }
  let (width, height) = probe_video_size(&ffprobe, &input)?;
  Ok(VideoDimensions { width, height })
}

fn looks_runnable(path: &Path) -> bool {
  if !path.is_file() {
    return false;
  }
  let mut command = Command::new(path);
  configure_subprocess(&mut command);
  command
    .arg("-version")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .map(|s| s.success())
    .unwrap_or(false)
}

fn resolve_bin(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
  if let Ok(dir) = app.path().executable_dir() {
    for file_name in [name.to_string(), format!("{name}.exe")] {
      let candidate = dir.join(file_name);
      if looks_runnable(&candidate) {
        return Ok(candidate);
      }
    }
  }

  let triple = env!("TARGET_TRIPLE");
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let mut candidates = vec![manifest_dir.join("binaries").join(format!("{name}-{triple}"))];
  if cfg!(windows) {
    candidates.push(
      manifest_dir
        .join("binaries")
        .join(format!("{name}-{triple}.exe")),
    );
  }
  for candidate in candidates {
    if looks_runnable(&candidate) {
      return Ok(candidate);
    }
  }

  for candidate in [
    name.to_string(),
    format!("/opt/homebrew/bin/{name}"),
    format!("/usr/local/bin/{name}"),
    format!("/usr/bin/{name}"),
  ] {
    let path = PathBuf::from(&candidate);
    if looks_runnable(&path)
      || (!candidate.contains('/') && {
        let mut command = Command::new(&candidate);
        configure_subprocess(&mut command);
        command
          .arg("-version")
          .stdout(Stdio::null())
          .stderr(Stdio::null())
          .status()
          .map(|s| s.success())
          .unwrap_or(false)
      })
    {
      return Ok(PathBuf::from(candidate));
    }
  }

  Err(format!("未找到 {name}"))
}

fn is_video_file(path: &Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .map(|ext| VIDEO_EXTS.iter().any(|x| x.eq_ignore_ascii_case(ext)))
    .unwrap_or(false)
}

fn should_skip_video_name(name: &str) -> bool {
  // 隐藏文件 + 原位压制临时输出 `.影工临时_*`
  name.starts_with('.')
}

fn should_skip_dir_name(name: &str) -> bool {
  name.is_empty()
    || name.starts_with('.')
    || name == "影工输出"
    || name == "快压输出"
    || name == "_compressed"
}

/// 自监控根起最多扫描的相对深度（根为 0，直接子目录为 1）。
const MAX_DRAMA_DEPTH: usize = 8;

fn relative_drama_name(root: &Path, dir: &Path) -> String {
  dir.strip_prefix(root)
    .unwrap_or(dir)
    .to_string_lossy()
    .replace('\\', "/")
}

fn is_strict_descendant(ancestor: &Path, maybe_child: &Path) -> bool {
  maybe_child.starts_with(ancestor) && maybe_child != ancestor
}

struct DirScan {
  videos: Vec<DramaVideo>,
  subdirs: Vec<PathBuf>,
}

/// 一次 read_dir：同时收集本层视频与子目录，避免每个目录扫两遍。
fn scan_dir_once(dir: &Path) -> DirScan {
  let Ok(entries) = fs::read_dir(dir) else {
    return DirScan {
      videos: vec![],
      subdirs: vec![],
    };
  };
  let mut videos = Vec::new();
  let mut subdirs = Vec::new();
  for entry in entries.flatten() {
    let file_type = match entry.file_type() {
      Ok(t) => t,
      Err(_) => continue,
    };
    let path = entry.path();
    let name = entry
      .file_name()
      .to_str()
      .unwrap_or("")
      .to_string();
    if file_type.is_dir() {
      if !should_skip_dir_name(&name) {
        subdirs.push(path);
      }
      continue;
    }
    if !file_type.is_file() || !is_video_file(&path) || should_skip_video_name(&name) {
      continue;
    }
    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
    videos.push(DramaVideo {
      name,
      path: path.to_string_lossy().to_string(),
      size,
    });
  }
  videos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  DirScan { videos, subdirs }
}

/// 递归收集「本层有视频」的目录；depth 为相对监控根的深度。
fn collect_drama_candidates(
  root: &Path,
  dir: &Path,
  depth: usize,
  out: &mut Vec<(PathBuf, Vec<DramaVideo>)>,
  dirs_scanned: &mut u32,
  progress: &mut impl FnMut(u32, u32, u32, &str),
) {
  if depth > MAX_DRAMA_DEPTH {
    return;
  }

  *dirs_scanned += 1;
  let scan = scan_dir_once(dir);
  if depth >= 1 && !scan.videos.is_empty() {
    out.push((dir.to_path_buf(), scan.videos));
  }

  let current = relative_drama_name(root, dir);
  let videos_found: u32 = out.iter().map(|(_, v)| v.len() as u32).sum();
  progress(*dirs_scanned, out.len() as u32, videos_found, &current);

  for path in scan.subdirs {
    collect_drama_candidates(root, &path, depth + 1, out, dirs_scanned, progress);
  }
}

/// 有更深「本层有视频」后代时，丢弃祖先候选。
fn prefer_deeper_dramas(
  candidates: Vec<(PathBuf, Vec<DramaVideo>)>,
) -> Vec<(PathBuf, Vec<DramaVideo>)> {
  candidates
    .iter()
    .filter(|(path, _)| {
      !candidates
        .iter()
        .any(|(other, _)| is_strict_descendant(path, other))
    })
    .cloned()
    .collect()
}

fn system_time_to_ms(t: std::time::SystemTime) -> u64 {
  t.duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

/// Folder age: birth → ctime (unix) → mtime.
fn folder_created_at_ms(path: &Path) -> u64 {
  let meta = match fs::metadata(path) {
    Ok(m) => m,
    Err(_) => return 0,
  };
  if let Ok(t) = meta.created() {
    return system_time_to_ms(t);
  }
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

fn collect_videos(dir: &Path) -> Vec<DramaVideo> {
  scan_dir_once(dir).videos
}

fn list_drama_folders_sync(
  watch_dir: String,
  mut on_progress: impl FnMut(DramaScanProgress),
) -> Result<Vec<DramaFolder>, String> {
  let root = PathBuf::from(&watch_dir);
  if !root.is_dir() {
    return Err(format!("监控目录不存在：{watch_dir}"));
  }

  let mut last_emit = std::time::Instant::now();
  let mut emit = |dirs_scanned: u32, dramas_found: u32, videos_found: u32, current: &str| {
    let due = dirs_scanned == 1 || dirs_scanned % 8 == 0 || last_emit.elapsed().as_millis() >= 200;
    if !due {
      return;
    }
    last_emit = std::time::Instant::now();
    on_progress(DramaScanProgress {
      dirs_scanned,
      dramas_found,
      videos_found,
      current_name: current.to_string(),
    });
  };

  let mut candidates = Vec::new();
  let mut dirs_scanned = 0_u32;
  collect_drama_candidates(
    &root,
    &root,
    0,
    &mut candidates,
    &mut dirs_scanned,
    &mut emit,
  );
  let kept = prefer_deeper_dramas(candidates);

  let mut folders = Vec::with_capacity(kept.len());
  for (path, videos) in kept {
    let name = relative_drama_name(&root, &path);
    if name.is_empty() {
      continue;
    }
    folders.push(DramaFolder {
      name,
      path: path.to_string_lossy().to_string(),
      video_count: videos.len(),
      created_at_ms: folder_created_at_ms(&path),
      videos,
    });
  }

  folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  let videos_found: u32 = folders.iter().map(|f| f.video_count as u32).sum();
  on_progress(DramaScanProgress {
    dirs_scanned,
    dramas_found: folders.len() as u32,
    videos_found,
    current_name: String::new(),
  });
  Ok(folders)
}

#[tauri::command]
pub async fn list_drama_folders(
  app: AppHandle,
  watch_dir: String,
) -> Result<Vec<DramaFolder>, String> {
  tauri::async_runtime::spawn_blocking(move || {
    list_drama_folders_sync(watch_dir, |payload| {
      let _ = app.emit("drama-scan-progress", payload);
    })
  })
  .await
  .map_err(|e| format!("扫描任务异常：{e}"))?
}

fn parse_out_time_secs(line: &str) -> Option<f64> {
  let line = line.trim();
  if let Some(value) = line.strip_prefix("out_time_us=") {
    return value.trim().parse::<f64>().ok().map(|us| us / 1_000_000.0);
  }
  if let Some(value) = line.strip_prefix("out_time_ms=") {
    let raw = value.trim();
    if raw == "N/A" {
      return None;
    }
    return raw.parse::<f64>().ok().map(|us| us / 1_000_000.0);
  }
  if let Some(value) = line.strip_prefix("out_time=") {
    let raw = value.trim();
    if raw == "N/A" {
      return None;
    }
    // 形如 00:01:23.456789
    let mut parts = raw.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    return Some(hours * 3600.0 + minutes * 60.0 + seconds);
  }
  None
}

fn probe_duration_secs(ffprobe: &Path, input: &Path) -> Option<f64> {
  let mut command = Command::new(ffprobe);
  configure_subprocess(&mut command);
  let output = command
    .args([
      "-v",
      "error",
      "-show_entries",
      "format=duration",
      "-of",
      "default=noprint_wrappers=1:nokey=1",
      input.to_string_lossy().as_ref(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  String::from_utf8_lossy(&output.stdout)
    .trim()
    .parse::<f64>()
    .ok()
    .filter(|v| v.is_finite() && *v > 0.0)
}

fn escape_concat_path(path: &str) -> String {
  path.replace('\\', "/").replace('\'', "'\\''")
}

fn ensure_mp4_extension(name: &str) -> String {
  let trimmed = name.trim();
  if trimmed.is_empty() {
    return trimmed.to_string();
  }
  if trimmed.len() >= 4 && trimmed[trimmed.len() - 4..].eq_ignore_ascii_case(".mp4") {
    return trimmed.to_string();
  }
  format!("{trimmed}.mp4")
}

#[tauri::command]
pub async fn merge_videos(
  app: AppHandle,
  state: State<'_, Arc<CompressState>>,
  id: String,
  input_paths: Vec<String>,
  output_dir: String,
  output_name: String,
  quality_preset: Option<String>,
  normalize_resolution: Option<bool>,
  cancel_key: Option<String>,
) -> Result<MergeResult, String> {
  if input_paths.len() < 2 {
    return Err("请至少选择两个视频进行合成".into());
  }

  let output_dir_path = PathBuf::from(&output_dir);
  if !output_dir_path.exists() {
    fs::create_dir_all(&output_dir_path).map_err(|e| format!("无法创建输出目录：{e}"))?;
  }

  let output_name = ensure_mp4_extension(&output_name);
  let output_path = output_dir_path.join(&output_name);
  let ffmpeg = resolve_bin(&app, "ffmpeg")?;
  let ffprobe = resolve_bin(&app, "ffprobe")?;
  let normalize = normalize_resolution.unwrap_or(false);

  let mut total_duration = 0.0_f64;
  let mut sizes: Vec<(u32, u32)> = Vec::new();
  let mut durations: Vec<f64> = Vec::new();
  let mut has_audio: Vec<bool> = Vec::new();
  let mut fingerprints: Vec<StreamFingerprint> = Vec::new();
  for path in &input_paths {
    let p = PathBuf::from(path);
    if !p.is_file() {
      return Err(format!("找不到输入文件：{path}"));
    }
    let fp = probe_stream_fingerprint(&ffprobe, &p)?;
    sizes.push((fp.width, fp.height));
    has_audio.push(fp.has_audio);
    fingerprints.push(fp);
    let secs = probe_duration_secs(&ffprobe, &p).unwrap_or(0.0);
    durations.push(secs);
    total_duration += secs;
  }

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
  let video_copy_ok = !mismatched && can_copy_video(&fingerprints);
  let full_copy_ok = video_copy_ok && can_stream_copy(&fingerprints);
  let all_have_audio = !has_audio.is_empty() && has_audio.iter().all(|v| *v);
  let cancel = Arc::clone(&state);
  let cancel_key = cancel_key.unwrap_or_else(|| id.clone());
  let app_for_progress = app.clone();
  let id_for_progress = id.clone();
  let output_path_clone = output_path.clone();
  let ffprobe_for_check = ffprobe.clone();
  let preset = quality_preset.unwrap_or_else(|| "size".into());
  let fps_arg = fps_filter_arg(
    fingerprints
      .first()
      .map(|fp| fp.r_frame_rate.as_str())
      .unwrap_or(""),
  );
  let input_paths_clone = input_paths.clone();

  let result = tauri::async_runtime::spawn_blocking(move || {
    let list_path = std::env::temp_dir().join(format!("kuaiya-concat-{id}.txt"));

    let write_concat_list = || -> Result<(), String> {
      let mut file =
        fs::File::create(&list_path).map_err(|e| format!("无法创建合成列表：{e}"))?;
      for path in &input_paths_clone {
        writeln!(file, "file '{}'", escape_concat_path(path))
          .map_err(|e| format!("写入合成列表失败：{e}"))?;
      }
      Ok(())
    };

    let run_once = |strategy: ConcatStrategy, encoder: VideoEncoderKind| -> Result<(), String> {
      let mut cmd = Command::new(&ffmpeg);
      configure_subprocess(&mut cmd);
      cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
      let mut map_audio = true;

      if uses_filter_concat(mismatched, strategy) {
        for path in &input_paths_clone {
          cmd.arg("-i").arg(path);
        }
        let encode_wh = if mismatched { target_wh } else { first_wh };
        let (filter, with_audio) = build_normalize_filter(
          input_paths_clone.len(),
          encode_wh.0,
          encode_wh.1,
          &has_audio,
          &durations,
          &fps_arg,
        );
        cmd.args(["-filter_complex", &filter, "-map", "[outv]"]);
        if with_audio {
          cmd.args(["-map", "[outa]"]);
        } else {
          map_audio = false;
        }
        append_video_encode_args(&mut cmd, encoder, &preset, encode_wh.0, encode_wh.1);
        if map_audio {
          append_audio_aac_args(&mut cmd, &preset);
        }
      } else {
        write_concat_list()?;
        cmd.args([
          "-f",
          "concat",
          "-safe",
          "0",
          "-i",
          list_path.to_string_lossy().as_ref(),
        ]);
        match strategy {
          ConcatStrategy::FullCopy => {
            cmd.args(["-c", "copy"]);
          }
          ConcatStrategy::VideoCopyAudioEncode => {
            cmd.args(["-c:v", "copy"]);
            append_audio_aac_unified_args(&mut cmd, &preset);
          }
          ConcatStrategy::NormalizeThenCopy => {
            return Err("内部错误：NormalizeThenCopy 未实现".into());
          }
          ConcatStrategy::FullReencode => {
            return Err("内部错误：重编码应走 filter concat".into());
          }
        }
      }

      cmd.args([
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        "-nostats",
        output_path_clone.to_string_lossy().as_ref(),
      ]);

      let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 FFmpeg 合成失败：{e}"))?;

      let stdout = child.stdout.take().ok_or_else(|| "无法读取进度".to_string())?;
      let stderr = child.stderr.take().ok_or_else(|| "无法读取错误输出".to_string())?;
      let stderr_worker = spawn_stderr_collector(stderr);
      let reader = BufReader::new(stdout);
      let mut last_progress = 0_u32;

      for line in reader.lines().flatten() {
        if cancel.is_cancelled(&cancel_key) {
          let _ = child.kill();
          let _ = stderr_worker.join();
          let _ = fs::remove_file(&list_path);
          let _ = fs::remove_file(&output_path_clone);
          return Err("已取消合成".into());
        }
        if let Some(out_secs) = parse_out_time_secs(&line) {
          let progress = if total_duration > 0.0 {
            (((out_secs / total_duration).clamp(0.0, 1.0) * 100.0).floor() as u32).min(99)
          } else if last_progress < 95 {
            last_progress + 1
          } else {
            last_progress
          };
          if progress > last_progress {
            last_progress = progress;
            let _ = app_for_progress.emit(
              "merge-progress",
              MergeProgressPayload {
                id: id_for_progress.clone(),
                progress,
              },
            );
          }
        }
      }

      let status = child.wait().map_err(|e| format!("等待合成结束失败：{e}"))?;
      let err_buf = stderr_worker.join().unwrap_or_default();
      let _ = fs::remove_file(&list_path);

      if !status.success() {
        let _ = fs::remove_file(&output_path_clone);
        let detail = err_buf
          .lines()
          .rev()
          .find(|l| !l.trim().is_empty())
          .unwrap_or("合成失败");
        return Err(format!("合成失败：{detail}"));
      }
      Ok(())
    };

    let reset_progress = || {
      let _ = fs::remove_file(&output_path_clone);
      let _ = app_for_progress.emit(
        "merge-progress",
        MergeProgressPayload {
          id: id_for_progress.clone(),
          progress: 0,
        },
      );
    };

    let needs_video_normalize =
      !mismatched && !indices_needing_video_normalize(&fingerprints).is_empty();
    let strategies = concat_strategies(
      mismatched,
      full_copy_ok,
      video_copy_ok,
      all_have_audio,
      needs_video_normalize,
    );

    let mut last_err = String::from("合成失败");
    let mut done = false;
    'strategy: for strategy in strategies {
      if cancel.is_cancelled(&cancel_key) {
        return Err("已取消合成".into());
      }

      let encoders = match strategy {
        ConcatStrategy::FullCopy
        | ConcatStrategy::VideoCopyAudioEncode
        | ConcatStrategy::NormalizeThenCopy => {
          vec![VideoEncoderKind::X264] // 占位，copy 路径不使用
        }
        ConcatStrategy::FullReencode => encoder_fallback_chain(&ffmpeg),
      };

      for encoder in encoders {
        match run_once(strategy, encoder) {
          Ok(()) => {
            done = true;
            break 'strategy;
          }
          Err(e) => {
            if cancel.is_cancelled(&cancel_key) || e.contains("已取消") {
              return Err(e);
            }
            last_err = e;
            reset_progress();
            // copy 类策略不轮询硬编；FullReencode 硬编失败继续软编
            if strategy != ConcatStrategy::FullReencode {
              break;
            }
            if encoder == VideoEncoderKind::X264 {
              break;
            }
            mark_hw_encoder_failed();
          }
        }
      }
    }

    if !done {
      return Err(last_err);
    }

    assert_resolution(&ffprobe_for_check, &output_path_clone, assert_wh)?;

    let output_size = fs::metadata(&output_path_clone)
      .map(|m| m.len())
      .unwrap_or(0);

    let _ = app_for_progress.emit(
      "merge-progress",
      MergeProgressPayload {
        id: id_for_progress,
        progress: 100,
      },
    );

    Ok(MergeResult {
      output_path: output_path_clone.to_string_lossy().to_string(),
      output_size,
    })
  })
  .await
  .map_err(|e| format!("合成任务异常：{e}"))?;

  result
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::time::{SystemTime, UNIX_EPOCH};

  #[cfg(unix)]
  use std::os::unix::fs::PermissionsExt;

  #[cfg(unix)]
  fn fake_ffprobe(video_stdout: &str, audio_stdout: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
      "video-compressor-fingerprint-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let ffprobe = dir.join("ffprobe");
    let script = format!(
      "#!/bin/sh\nfor a in \"$@\"; do\n  case \"$a\" in\n    v:0) printf '%s\\n' '{video}'\n         exit 0 ;;\n    a:0) printf '%s\\n' '{audio}'\n         exit 0 ;;\n  esac\ndone\nexit 1\n",
      video = video_stdout.replace('\'', "'\\''"),
      audio = audio_stdout.replace('\'', "'\\''"),
    );
    fs::write(&ffprobe, script).unwrap();
    let mut permissions = fs::metadata(&ffprobe).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ffprobe, permissions).unwrap();
    (dir, ffprobe)
  }

  #[test]
  fn skips_temp_and_hidden_video_names() {
    assert!(should_skip_video_name(".影工临时_01_abcd.mp4"));
    assert!(should_skip_video_name(".hidden.mp4"));
    assert!(!should_skip_video_name("01.mp4"));
    assert!(!should_skip_video_name("episode.mp4"));
  }

  #[test]
  fn folder_created_at_ms_reads_existing_dir() {
    let dir = std::env::temp_dir().join(format!(
      "video-compressor-folder-age-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let ms = folder_created_at_ms(&dir);
    assert!(ms > 0, "expected nonzero folder age, got {ms}");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn collect_videos_excludes_temp_outputs() {
    let dir = std::env::temp_dir().join(format!(
      "video-compressor-collect-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("01.mp4"), [1]).unwrap();
    fs::write(dir.join(".影工临时_01_x.mp4"), [1]).unwrap();
    let videos = collect_videos(&dir);
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].name, "01.mp4");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn list_drama_folders_uses_relative_nested_names() {
    let root = std::env::temp_dir().join(format!(
      "video-compressor-deep-name-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let nested = root.join("剧A").join("en");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("01.mp4"), [1]).unwrap();

    let folders = list_drama_folders_sync(root.to_string_lossy().to_string(), |_| {}).unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "剧A/en");
    assert_eq!(folders[0].video_count, 1);

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn list_drama_folders_drops_ancestor_when_deeper_has_videos() {
    let root = std::env::temp_dir().join(format!(
      "video-compressor-deep-prefer-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let parent = root.join("剧A");
    let child = parent.join("en");
    fs::create_dir_all(&child).unwrap();
    fs::write(parent.join("root-level.mp4"), [1]).unwrap();
    fs::write(child.join("01.mp4"), [1]).unwrap();

    let folders = list_drama_folders_sync(root.to_string_lossy().to_string(), |_| {}).unwrap();
    let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["剧A/en"]);
    assert!(!names.iter().any(|n| *n == "剧A"));

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn list_drama_folders_skips_excluded_dir_names_at_any_depth() {
    let root = std::env::temp_dir().join(format!(
      "video-compressor-deep-skip-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let excluded = root.join("剧A").join("影工输出");
    fs::create_dir_all(&excluded).unwrap();
    fs::write(excluded.join("01.mp4"), [1]).unwrap();
    let ok = root.join("剧B");
    fs::create_dir_all(&ok).unwrap();
    fs::write(ok.join("01.mp4"), [1]).unwrap();

    let folders = list_drama_folders_sync(root.to_string_lossy().to_string(), |_| {}).unwrap();
    let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["剧B"]);

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn list_drama_folders_reports_progress_counts() {
    let root = std::env::temp_dir().join(format!(
      "video-compressor-scan-progress-{}-{}",
      std::process::id(),
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    let nested = root.join("剧A").join("en");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("01.mp4"), [1]).unwrap();
    fs::write(nested.join("02.mp4"), [1]).unwrap();

    let mut reports = Vec::new();
    let folders = list_drama_folders_sync(root.to_string_lossy().to_string(), |p| {
      reports.push(p);
    })
    .unwrap();

    assert_eq!(folders.len(), 1);
    assert!(!reports.is_empty());
    let last = reports.last().unwrap();
    assert!(last.dirs_scanned >= 3, "root + 剧A + en, got {}", last.dirs_scanned);
    assert_eq!(last.dramas_found, 1);
    assert_eq!(last.videos_found, 2);

    let _ = fs::remove_dir_all(&root);
  }

  fn fingerprint(r_frame_rate: &str, time_base: &str) -> StreamFingerprint {
    StreamFingerprint {
      width: 1080,
      height: 1920,
      video_codec: "h264".into(),
      pix_fmt: "yuv420p".into(),
      video_profile: "high".into(),
      r_frame_rate: r_frame_rate.into(),
      time_base: time_base.into(),
      has_audio: true,
      audio_codec: "aac".into(),
      sample_rate: "48000".into(),
      channels: "2".into(),
    }
  }

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

  #[test]
  fn can_copy_video_rejects_mismatched_frame_rate() {
    let a = fingerprint("25/1", "1/12800");
    let b = fingerprint("24/1", "1/12800");
    assert!(
      !can_copy_video(&[a, b]),
      "24fps mixed with 25fps must re-encode"
    );
  }

  #[test]
  fn can_copy_video_rejects_mismatched_time_base() {
    let a = fingerprint("25/1", "1/12800");
    let b = fingerprint("25/1", "1/12288");
    assert!(
      !can_copy_video(&[a, b]),
      "different time_base must re-encode"
    );
  }

  #[test]
  fn can_copy_video_rejects_empty_timing() {
    let a = fingerprint("", "");
    let b = fingerprint("", "");
    assert!(
      !can_copy_video(&[a, b]),
      "missing fps/time_base must not stream-copy"
    );
  }

  #[test]
  fn can_copy_video_allows_matching_timing() {
    let a = fingerprint("25/1", "1/12800");
    let b = fingerprint("25/1", "1/12800");
    assert!(can_copy_video(&[a, b]));
  }

  #[cfg(unix)]
  #[test]
  fn probe_stream_fingerprint_reads_frame_rate_and_time_base() {
    let video = [
      "codec_name=h264",
      "width=1080",
      "height=1920",
      "pix_fmt=yuv420p",
      "profile=High",
      "r_frame_rate=24/1",
      "time_base=1/12288",
    ]
    .join("\n");
    let audio = ["codec_name=aac", "sample_rate=48000", "channels=2"].join("\n");
    let (dir, ffprobe) = fake_ffprobe(&video, &audio);
    let input = dir.join("clip.mp4");
    fs::write(&input, [1]).unwrap();

    let fp = probe_stream_fingerprint(&ffprobe, &input).unwrap();
    assert_eq!(fp.r_frame_rate, "24/1");
    assert_eq!(fp.time_base, "1/12288");
    assert_eq!(fp.width, 1080);
    assert_eq!(fp.height, 1920);

    fs::remove_dir_all(dir).unwrap();
  }

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

  #[test]
  fn filter_concat_used_when_full_reencode_even_if_resolution_matches() {
    assert!(uses_filter_concat(false, ConcatStrategy::FullReencode));
    assert!(!uses_filter_concat(false, ConcatStrategy::FullCopy));
    assert!(!uses_filter_concat(
      false,
      ConcatStrategy::VideoCopyAudioEncode
    ));
    assert!(uses_filter_concat(true, ConcatStrategy::FullReencode));
  }

  #[test]
  fn normalize_filter_unifies_fps_and_resets_timestamps() {
    let (filter, has_audio) =
      build_normalize_filter(2, 1080, 1920, &[true, true], &[1.0, 1.0], "25/1");
    assert!(has_audio);
    assert!(
      filter.contains("fps=25/1"),
      "expected fps unification, got {filter}"
    );
    assert!(filter.contains("setpts=PTS-STARTPTS"));
    assert!(filter.contains("concat=n=2:v=1:a=1"));
  }

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
}
