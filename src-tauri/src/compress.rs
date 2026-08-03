use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressProgressPayload {
  pub id: String,
  pub progress: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressResult {
  pub output_path: String,
  pub output_size: u64,
}

pub struct CompressState {
  pub cancel: AtomicBool,
}

impl Default for CompressState {
  fn default() -> Self {
    Self {
      cancel: AtomicBool::new(false),
    }
  }
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
  // 1) 安装包内：与可执行文件同目录（externalBin sidecar）
  if let Ok(dir) = app.path().executable_dir() {
    for file_name in [name.to_string(), format!("{name}.exe")] {
      let candidate = dir.join(file_name);
      if looks_runnable(&candidate) {
        return Ok(candidate);
      }
    }
  }

  // 2) 开发态：src-tauri/binaries/<name>-<target-triple>
  let triple = env!("TARGET_TRIPLE");
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let mut dev_candidates = vec![manifest_dir.join("binaries").join(format!("{name}-{triple}"))];
  if cfg!(windows) {
    dev_candidates.push(
      manifest_dir
        .join("binaries")
        .join(format!("{name}-{triple}.exe")),
    );
  }
  for candidate in dev_candidates {
    if looks_runnable(&candidate) {
      return Ok(candidate);
    }
  }

  // 3) 本机 PATH / 常见安装位置（仅方便本地开发）
  let system_candidates = [
    name.to_string(),
    format!("/opt/homebrew/bin/{name}"),
    format!("/usr/local/bin/{name}"),
    format!("/usr/bin/{name}"),
  ];
  for candidate in system_candidates {
    let path = PathBuf::from(&candidate);
    if looks_runnable(&path) {
      return Ok(path);
    }
    // PATH 中的命令名
    if !candidate.contains('/')
      && Command::new(&candidate)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
      return Ok(PathBuf::from(candidate));
    }
  }

  Err(format!(
    "未找到 {name}。打包前请执行 npm run prepare:ffmpeg；开发调试也可 brew install ffmpeg"
  ))
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

/// 从 FFmpeg `-progress` 输出解析已处理时长（秒）。
/// 注意：历史字段 `out_time_ms` 的单位实际是微秒，不是毫秒。
fn parse_out_time_secs(line: &str) -> Option<f64> {
  let line = line.trim();

  if let Some(value) = line.strip_prefix("out_time_us=") {
    let us: f64 = value.trim().parse().ok()?;
    return Some(us / 1_000_000.0);
  }

  // FFmpeg 文档：out_time_ms 名义叫 ms，实际是 microseconds（已弃用）
  if let Some(value) = line.strip_prefix("out_time_ms=") {
    let raw = value.trim();
    if raw == "N/A" {
      return None;
    }
    let us: f64 = raw.parse().ok()?;
    return Some(us / 1_000_000.0);
  }

  if let Some(value) = line.strip_prefix("out_time=") {
    let raw = value.trim();
    if raw == "N/A" {
      return None;
    }
    return parse_timestamp_secs(raw);
  }

  None
}

fn parse_timestamp_secs(value: &str) -> Option<f64> {
  // 形如 00:01:23.456789
  let mut parts = value.split(':');
  let hours: f64 = parts.next()?.parse().ok()?;
  let minutes: f64 = parts.next()?.parse().ok()?;
  let seconds: f64 = parts.next()?.parse().ok()?;
  Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

#[tauri::command]
pub async fn compress_video(
  app: AppHandle,
  state: State<'_, Arc<CompressState>>,
  id: String,
  input_path: String,
  output_dir: String,
  output_name: String,
) -> Result<CompressResult, String> {
  if input_path.starts_with("browser://") {
    return Err("浏览器模式无法写出文件，请使用桌面应用并通过「选择文件」添加视频".into());
  }

  let input = PathBuf::from(&input_path);
  if !input.is_file() {
    return Err(format!("找不到输入文件：{input_path}"));
  }

  let output_dir_path = PathBuf::from(&output_dir);
  if !output_dir_path.exists() {
    fs::create_dir_all(&output_dir_path).map_err(|e| format!("无法创建输出目录：{e}"))?;
  }
  if !output_dir_path.is_dir() {
    return Err(format!("输出路径不是文件夹：{output_dir}"));
  }

  let output_path = output_dir_path.join(&output_name);
  let ffmpeg = resolve_bin(&app, "ffmpeg")?;
  let ffprobe = resolve_bin(&app, "ffprobe")?;
  let duration = probe_duration_secs(&ffprobe, &input);
  let cancel = Arc::clone(&state);
  // 不在单任务开始时重置 cancel，避免并行任务互相干扰

  let app_for_progress = app.clone();
  let id_for_progress = id.clone();
  let output_path_clone = output_path.clone();

  let result = tauri::async_runtime::spawn_blocking(move || {
    let mut child = Command::new(&ffmpeg)
      .args([
        "-y",
        "-i",
        input.to_string_lossy().as_ref(),
        // 画质优先：较低 CRF，不缩放分辨率
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
      .map_err(|e| format!("启动 FFmpeg 失败：{e}"))?;

    let stdout = child
      .stdout
      .take()
      .ok_or_else(|| "无法读取 FFmpeg 进度输出".to_string())?;
    let mut stderr = child
      .stderr
      .take()
      .ok_or_else(|| "无法读取 FFmpeg 错误输出".to_string())?;

    let reader = BufReader::new(stdout);
    let mut last_progress = 0_u32;

    for line in reader.lines().flatten() {
      if cancel.cancel.load(Ordering::SeqCst) {
        let _ = child.kill();
        return Err("已取消压缩".into());
      }

      if let Some(out_secs) = parse_out_time_secs(&line) {
        let progress = match duration {
          Some(total) if total > 0.0 => {
            let ratio = (out_secs / total).clamp(0.0, 1.0);
            // 编码未结束前最多到 99，完成时再设 100
            ((ratio * 100.0).floor() as u32).min(99)
          }
          // 拿不到总时长时用不确定进度，缓慢爬升，避免瞬间顶满
          _ => {
            if last_progress < 95 {
              last_progress.saturating_add(1)
            } else {
              last_progress
            }
          }
        };

        if progress > last_progress {
          last_progress = progress;
          let _ = app_for_progress.emit(
            "compress-progress",
            CompressProgressPayload {
              id: id_for_progress.clone(),
              progress,
            },
          );
        }
      }
    }

    let status = child
      .wait()
      .map_err(|e| format!("等待 FFmpeg 结束失败：{e}"))?;

    if !status.success() {
      let mut err_buf = String::new();
      let _ = stderr.read_to_string(&mut err_buf);
      let detail = err_buf
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("FFmpeg 压缩失败");
      return Err(format!("压缩失败：{detail}"));
    }

    if !output_path_clone.is_file() {
      return Err("压缩完成但未找到输出文件".into());
    }

    let output_size = fs::metadata(&output_path_clone)
      .map(|m| m.len())
      .unwrap_or(0);

    let _ = app_for_progress.emit(
      "compress-progress",
      CompressProgressPayload {
        id: id_for_progress,
        progress: 100,
      },
    );

    Ok(CompressResult {
      output_path: output_path_clone.to_string_lossy().to_string(),
      output_size,
    })
  })
  .await
  .map_err(|e| format!("压缩任务异常：{e}"))?;

  result
}

#[tauri::command]
pub fn prepare_compress_batch(state: State<'_, Arc<CompressState>>) {
  state.cancel.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub fn cancel_compress(state: State<'_, Arc<CompressState>>) {
  state.cancel.store(true, Ordering::SeqCst);
}
