use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::compress::CompressState;

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

  let mut total_duration = 0.0_f64;
  for path in &input_paths {
    let p = PathBuf::from(path);
    if !p.is_file() {
      return Err(format!("找不到输入文件：{path}"));
    }
    if let Some(secs) = probe_duration_secs(&ffprobe, &p) {
      total_duration += secs;
    }
  }

  let list_path = std::env::temp_dir().join(format!("kuaiya-concat-{id}.txt"));
  {
    let mut file = fs::File::create(&list_path).map_err(|e| format!("无法创建合成列表：{e}"))?;
    for path in &input_paths {
      writeln!(file, "file '{}'", escape_concat_path(path))
        .map_err(|e| format!("写入合成列表失败：{e}"))?;
    }
  }

  let cancel = Arc::clone(&state);
  let app_for_progress = app.clone();
  let id_for_progress = id.clone();
  let output_path_clone = output_path.clone();
  let list_path_clone = list_path.clone();

  let result = tauri::async_runtime::spawn_blocking(move || {
    let mut child = Command::new(&ffmpeg)
      .args([
        "-y",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        list_path_clone.to_string_lossy().as_ref(),
        "-c:v",
        "libx264",
        "-crf",
        "18",
        "-preset",
        "slow",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        "-nostats",
        output_path_clone.to_string_lossy().as_ref(),
      ])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .map_err(|e| format!("启动 FFmpeg 合成失败：{e}"))?;

    let stdout = child.stdout.take().ok_or_else(|| "无法读取进度".to_string())?;
    let mut stderr = child.stderr.take().ok_or_else(|| "无法读取错误输出".to_string())?;
    let reader = BufReader::new(stdout);
    let mut last_progress = 0_u32;

    for line in reader.lines().flatten() {
      if cancel.cancel.load(Ordering::SeqCst) {
        let _ = child.kill();
        let _ = fs::remove_file(&list_path_clone);
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
    let _ = fs::remove_file(&list_path_clone);

    if !status.success() {
      let mut err_buf = String::new();
      let _ = stderr.read_to_string(&mut err_buf);
      let detail = err_buf
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("合成失败");
      return Err(format!("合成失败：{detail}"));
    }

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
