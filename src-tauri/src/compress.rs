use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};

/// Suppress console windows for console subprocesses launched by the Windows GUI app.
pub fn configure_subprocess(command: &mut Command) {
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
  }

  #[cfg(not(windows))]
  let _ = command;
}

/// 后台持续排空 stderr，避免管道写满导致 FFmpeg 阻塞卡死。
pub fn spawn_stderr_collector(stderr: impl Read + Send + 'static) -> JoinHandle<String> {
  std::thread::spawn(move || {
    let mut buf = String::new();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
      line.clear();
      match reader.read_line(&mut line) {
        Ok(0) => break,
        Ok(_) => {
          // 只保留尾部，避免异常刷屏占内存
          if buf.len() + line.len() > 64 * 1024 {
            let keep_from = buf.len().saturating_sub(32 * 1024);
            buf = buf.split_off(keep_from);
          }
          buf.push_str(&line);
        }
        Err(_) => break,
      }
    }
    buf
  })
}

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
  /// 按任务 cancelKey 取消，互不影响其它任务
  cancelled: Mutex<HashSet<String>>,
}

impl CompressState {
  pub fn request_cancel(&self, key: &str) {
    if key.is_empty() {
      return;
    }
    if let Ok(mut set) = self.cancelled.lock() {
      set.insert(key.to_string());
    }
  }

  pub fn clear_cancel(&self, key: &str) {
    if key.is_empty() {
      return;
    }
    if let Ok(mut set) = self.cancelled.lock() {
      set.remove(key);
    }
  }

  pub fn is_cancelled(&self, key: &str) -> bool {
    if key.is_empty() {
      return false;
    }
    self.cancelled
      .lock()
      .map(|set| set.contains(key))
      .unwrap_or(false)
  }
}

/// 读取视频流宽高（coded width/height）。
pub fn probe_video_size(ffprobe: &Path, input: &Path) -> Result<(u32, u32), String> {
  let mut command = Command::new(ffprobe);
  configure_subprocess(&mut command);
  let output = command
    .args([
      "-v",
      "error",
      "-select_streams",
      "v:0",
      "-show_entries",
      "stream=width,height",
      "-of",
      "csv=p=0:s=x",
      input.to_string_lossy().as_ref(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output()
    .map_err(|e| format!("探测分辨率失败：{e}"))?;

  if !output.status.success() {
    return Err("无法读取视频分辨率".into());
  }

  let text = String::from_utf8_lossy(&output.stdout);
  let line = text
    .lines()
    .map(str::trim)
    .find(|l| !l.is_empty())
    .ok_or_else(|| "无法读取视频分辨率".to_string())?;

  let (w, h) = line
    .split_once('x')
    .ok_or_else(|| format!("分辨率格式无效：{line}"))?;
  let width: u32 = w
    .trim()
    .parse()
    .map_err(|_| format!("无效宽度：{w}"))?;
  let height: u32 = h
    .trim()
    .parse()
    .map_err(|_| format!("无效高度：{h}"))?;
  if width == 0 || height == 0 {
    return Err("视频分辨率为 0".into());
  }
  Ok((width, height))
}

/// 校验输出与期望分辨率一致（压制/合成均须保持原分辨率）。
pub fn assert_resolution(
  ffprobe: &Path,
  path: &Path,
  expected: (u32, u32),
) -> Result<(), String> {
  let got = probe_video_size(ffprobe, path)?;
  if got != expected {
    return Err(format!(
      "输出分辨率被改变：期望 {}×{}，实际 {}×{}（已保证不缩放，请重试或检查源文件）",
      expected.0, expected.1, got.0, got.1
    ));
  }
  Ok(())
}

fn is_compressed_output_valid_with_probe(ffprobe: &Path, path: &Path) -> bool {
  let is_non_empty_file = fs::metadata(path)
    .map(|metadata| metadata.is_file() && metadata.len() > 0)
    .unwrap_or(false);
  is_non_empty_file && probe_video_size(ffprobe, path).is_ok()
}

#[tauri::command]
pub async fn is_compressed_output_valid(app: AppHandle, path: String) -> bool {
  let Ok(ffprobe) = resolve_bin(&app, "ffprobe") else {
    return false;
  };
  let output = PathBuf::from(path);

  tauri::async_runtime::spawn_blocking(move || {
    is_compressed_output_valid_with_probe(&ffprobe, &output)
  })
  .await
  .unwrap_or(false)
}

impl Default for CompressState {
  fn default() -> Self {
    Self {
      cancelled: Mutex::new(HashSet::new()),
    }
  }
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
    if !candidate.contains('/') {
      let mut command = Command::new(&candidate);
      configure_subprocess(&mut command);
      if command
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
  }

  Err(format!(
    "未找到 {name}。打包前请执行 npm run prepare:ffmpeg；开发调试也可 brew install ffmpeg"
  ))
}

/// 应用异常退出 / 热重载后，原先 spawn 的 FFmpeg 可能成孤儿进程继续跑。
/// 启动时按本应用捆绑的 ffmpeg 路径清理残留，避免任务已中断但编码仍占 CPU。
pub fn cleanup_orphan_ffmpeg(app: &AppHandle) {
  let Ok(ffmpeg) = resolve_bin(app, "ffmpeg") else {
    return;
  };
  let path = ffmpeg.to_string_lossy().to_string();
  if path.is_empty() {
    return;
  }
  #[cfg(unix)]
  {
    // 只匹配本应用二进制路径，避免误杀系统 ffmpeg
    let _ = Command::new("pkill")
      .args(["-f", &path])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status();
  }
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
  quality_preset: Option<String>,
  // 任务级取消键；同一任务内并行视频共享，互不影响其它任务
  cancel_key: Option<String>,
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
  let inplace = output_path == input;
  let encode_path = if inplace {
    crate::replace::temp_output_path(&output_path)
  } else {
    output_path.clone()
  };
  let ffmpeg = resolve_bin(&app, "ffmpeg")?;
  let ffprobe = resolve_bin(&app, "ffprobe")?;
  let input_wh = probe_video_size(&ffprobe, &input)?;
  let duration = probe_duration_secs(&ffprobe, &input);
  let cancel = Arc::clone(&state);
  let cancel_key = cancel_key.unwrap_or_else(|| id.clone());

  let app_for_progress = app.clone();
  let id_for_progress = id.clone();
  let encode_path_clone = encode_path.clone();
  let final_path_clone = output_path.clone();
  let ffprobe_for_check = ffprobe.clone();
  let preset = quality_preset.unwrap_or_else(|| "size".into());

  let result = tauri::async_runtime::spawn_blocking(move || {
    use crate::encode::{
      append_audio_aac_args, append_video_encode_args, encoder_fallback_chain,
      mark_hw_encoder_failed, VideoEncoderKind,
    };
    use crate::replace::replace_file_force;

    let encoders = encoder_fallback_chain(&ffmpeg);
    let mut last_err = String::from("压缩失败");

    for (attempt, encoder) in encoders.into_iter().enumerate() {
      if cancel.is_cancelled(&cancel_key) {
        let _ = fs::remove_file(&encode_path_clone);
        return Err("已取消压缩".into());
      }
      if attempt > 0 {
        let _ = fs::remove_file(&encode_path_clone);
        let _ = app_for_progress.emit(
          "compress-progress",
          CompressProgressPayload {
            id: id_for_progress.clone(),
            progress: 0,
          },
        );
      }

      let mut cmd = Command::new(&ffmpeg);
      configure_subprocess(&mut cmd);
      cmd.args([
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        input.to_string_lossy().as_ref(),
      ]);
      // 不传 -s / -vf scale：分辨率与源一致
      append_video_encode_args(&mut cmd, encoder, &preset, input_wh.0, input_wh.1);
      append_audio_aac_args(&mut cmd, &preset);
      cmd.args([
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        "-nostats",
        encode_path_clone.to_string_lossy().as_ref(),
      ]);

      let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 FFmpeg 失败：{e}"))?;

      let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 FFmpeg 进度输出".to_string())?;
      let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "无法读取 FFmpeg 错误输出".to_string())?;
      let stderr_worker = spawn_stderr_collector(stderr);

      let reader = BufReader::new(stdout);
      let mut last_progress = 0_u32;

      for line in reader.lines().flatten() {
        if cancel.is_cancelled(&cancel_key) {
          let _ = child.kill();
          let _ = stderr_worker.join();
          let _ = fs::remove_file(&encode_path_clone);
          return Err("已取消压缩".into());
        }

        if let Some(out_secs) = parse_out_time_secs(&line) {
          let progress = match duration {
            Some(total) if total > 0.0 => {
              let ratio = (out_secs / total).clamp(0.0, 1.0);
              ((ratio * 100.0).floor() as u32).min(99)
            }
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
      let err_buf = stderr_worker.join().unwrap_or_default();

      if cancel.is_cancelled(&cancel_key) {
        let _ = fs::remove_file(&encode_path_clone);
        return Err("已取消压缩".into());
      }

      if status.success() && encode_path_clone.is_file() {
        if let Err(e) = assert_resolution(&ffprobe_for_check, &encode_path_clone, input_wh) {
          last_err = e;
          let _ = fs::remove_file(&encode_path_clone);
          // 硬编分辨率异常时继续回退；软编失败则直接返回
          if encoder == VideoEncoderKind::X264 {
            return Err(last_err);
          }
          continue;
        }

        if inplace {
          replace_file_force(&encode_path_clone, &final_path_clone)?;
        }

        let output_size = fs::metadata(&final_path_clone)
          .map(|m| m.len())
          .unwrap_or(0);

        let _ = app_for_progress.emit(
          "compress-progress",
          CompressProgressPayload {
            id: id_for_progress,
            progress: 100,
          },
        );

        return Ok(CompressResult {
          output_path: final_path_clone.to_string_lossy().to_string(),
          output_size,
        });
      }

      let detail = err_buf
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("FFmpeg 压缩失败");
      last_err = format!("压缩失败（{}）：{detail}", encoder.ffmpeg_name());
      let _ = fs::remove_file(&encode_path_clone);

      if encoder == VideoEncoderKind::X264 {
        break;
      }
      // 硬编失败：本进程后续任务直接软编，避免每个文件都先失败一次
      mark_hw_encoder_failed();
    }

    let _ = fs::remove_file(&encode_path_clone);
    Err(last_err)
  })
  .await
  .map_err(|e| format!("压缩任务异常：{e}"))?;

  result
}

#[tauri::command]
pub fn prepare_compress_batch(state: State<'_, Arc<CompressState>>, cancel_key: String) {
  state.clear_cancel(&cancel_key);
}

#[tauri::command]
pub fn cancel_compress(state: State<'_, Arc<CompressState>>, cancel_key: String) {
  state.request_cancel(&cancel_key);
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  #[cfg(unix)]
  use std::os::unix::fs::PermissionsExt;
  #[cfg(windows)]
  use std::process::Command;

  const MINIMAL_Y4M_VIDEO: &[u8] =
    b"YUV4MPEG2 W2 H2 F1:1 Ip A1:1 C420jpeg\nFRAME\n\x10\x10\x10\x10\x80\x80";

  #[cfg(unix)]
  fn fake_ffprobe(stdout: &str, succeeds: bool) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
      "video-compressor-output-validation-{}-{}",
      std::process::id(),
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let ffprobe = dir.join("ffprobe");
    fs::write(
      &ffprobe,
      format!(
        "#!/bin/sh\nprintf '%s\\n' '{}'\nexit {}\n",
        stdout,
        if succeeds { 0 } else { 1 }
      ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&ffprobe).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&ffprobe, permissions).unwrap();
    (dir, ffprobe)
  }

  #[cfg(unix)]
  #[test]
  fn validates_only_non_empty_outputs_with_readable_video_streams() {
    let (dir, ffprobe) = fake_ffprobe("1920x1080", true);
    let output = dir.join("output.mp4");

    assert!(!is_compressed_output_valid_with_probe(&ffprobe, &output));

    fs::write(&output, []).unwrap();
    assert!(!is_compressed_output_valid_with_probe(&ffprobe, &output));

    fs::write(&output, MINIMAL_Y4M_VIDEO).unwrap();
    assert!(is_compressed_output_valid_with_probe(&ffprobe, &output));

    fs::remove_dir_all(dir).unwrap();
  }

  #[cfg(unix)]
  #[test]
  fn rejects_non_empty_outputs_when_ffprobe_cannot_read_a_video_stream() {
    let (dir, ffprobe) = fake_ffprobe("", false);
    let output = dir.join("output.mp4");
    fs::write(&output, [1]).unwrap();

    assert!(!is_compressed_output_valid_with_probe(&ffprobe, &output));

    fs::remove_dir_all(dir).unwrap();
  }

  #[cfg(windows)]
  #[test]
  fn configures_a_windows_subprocess() {
    let mut command = Command::new("cmd");
    configure_subprocess(&mut command);
  }
}
