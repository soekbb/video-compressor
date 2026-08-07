use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::compress::{
  assert_resolution, probe_video_size, spawn_stderr_collector, CompressState,
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
  pub videos: Vec<DramaVideo>,
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct StreamFingerprint {
  width: u32,
  height: u32,
  video_codec: String,
  pix_fmt: String,
  video_profile: String,
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
  let output = Command::new(ffprobe)
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
    "stream=codec_name,width,height,pix_fmt,profile",
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
  })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConcatStrategy {
  /// 音视频全部 copy
  FullCopy,
  /// 视频 copy，音频统一重编码为 AAC 48k 立体声
  VideoCopyAudioEncode,
  /// 全量重编码（硬编优先）
  FullReencode,
}

/// 将各片段缩放到同一分辨率后 concat（用于分辨率不一致时）。
fn build_normalize_filter(
  count: usize,
  width: u32,
  height: u32,
  has_audio: &[bool],
  durations: &[f64],
) -> (String, bool) {
  let any_audio = has_audio.iter().any(|v| *v);
  let mut parts: Vec<String> = Vec::new();

  for i in 0..count {
    parts.push(format!(
      "[{i}:v]scale={width}:{height}:flags=bicubic,setsar=1,format=yuv420p,setpts=PTS-STARTPTS[v{i}]"
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
  Command::new(path)
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
      || (!candidate.contains('/')
        && Command::new(&candidate)
          .arg("-version")
          .stdout(Stdio::null())
          .stderr(Stdio::null())
          .status()
          .map(|s| s.success())
          .unwrap_or(false))
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

fn collect_videos(dir: &Path) -> Vec<DramaVideo> {
  let Ok(entries) = fs::read_dir(dir) else {
    return vec![];
  };
  let mut videos = Vec::new();
  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() || !is_video_file(&path) {
      continue;
    }
    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let name = path
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or("video")
      .to_string();
    videos.push(DramaVideo {
      name,
      path: path.to_string_lossy().to_string(),
      size,
    });
  }
  videos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  videos
}

#[tauri::command]
pub fn list_drama_folders(watch_dir: String) -> Result<Vec<DramaFolder>, String> {
  let root = PathBuf::from(&watch_dir);
  if !root.is_dir() {
    return Err(format!("监控目录不存在：{watch_dir}"));
  }

  let mut folders = Vec::new();
  let entries = fs::read_dir(&root).map_err(|e| format!("无法读取监控目录：{e}"))?;

  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_dir() {
      continue;
    }
    let name = path
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or("")
      .to_string();
    if name.is_empty()
      || name.starts_with('.')
      || name == "影工输出"
      || name == "快压输出"
      || name == "_compressed"
    {
      continue;
    }

    let videos = collect_videos(&path);
    if videos.is_empty() {
      continue;
    }

    folders.push(DramaFolder {
      name: name.clone(),
      path: path.to_string_lossy().to_string(),
      video_count: videos.len(),
      videos,
    });
  }

  folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  Ok(folders)
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
  let output = Command::new(ffprobe)
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
  let none_have_audio = has_audio.iter().all(|v| !*v);
  let cancel = Arc::clone(&state);
  let cancel_key = cancel_key.unwrap_or_else(|| id.clone());
  let app_for_progress = app.clone();
  let id_for_progress = id.clone();
  let output_path_clone = output_path.clone();
  let ffprobe_for_check = ffprobe.clone();
  let preset = quality_preset.unwrap_or_else(|| "size".into());
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
      cmd.args(["-y", "-hide_banner", "-loglevel", "error"]);
      let mut map_audio = true;

      if mismatched {
        for path in &input_paths_clone {
          cmd.arg("-i").arg(path);
        }
        let (filter, with_audio) = build_normalize_filter(
          input_paths_clone.len(),
          target_wh.0,
          target_wh.1,
          &has_audio,
          &durations,
        );
        cmd.args(["-filter_complex", &filter, "-map", "[outv]"]);
        if with_audio {
          cmd.args(["-map", "[outa]"]);
        } else {
          map_audio = false;
        }
        append_video_encode_args(&mut cmd, encoder, &preset, target_wh.0, target_wh.1);
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
          ConcatStrategy::FullReencode => {
            append_video_encode_args(
              &mut cmd,
              encoder,
              &preset,
              first_wh.0,
              first_wh.1,
            );
            if none_have_audio {
              cmd.arg("-an");
            } else {
              append_audio_aac_unified_args(&mut cmd, &preset);
            }
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

    let mut strategies: Vec<ConcatStrategy> = Vec::new();
    if mismatched {
      strategies.push(ConcatStrategy::FullReencode);
    } else {
      if full_copy_ok {
        strategies.push(ConcatStrategy::FullCopy);
      }
      // 视频一致、音频不一致；或全 copy 失败后再试「视频 copy + 转音频」
      if all_have_audio && video_copy_ok {
        strategies.push(ConcatStrategy::VideoCopyAudioEncode);
      }
      strategies.push(ConcatStrategy::FullReencode);
    }

    let mut last_err = String::from("合成失败");
    let mut done = false;
    'strategy: for strategy in strategies {
      if cancel.is_cancelled(&cancel_key) {
        return Err("已取消合成".into());
      }

      let encoders = match strategy {
        ConcatStrategy::FullCopy | ConcatStrategy::VideoCopyAudioEncode => {
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
