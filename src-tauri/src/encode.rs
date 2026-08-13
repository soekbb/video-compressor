use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::compress::configure_subprocess;
use serde::Serialize;

/// 连续硬编失败达到此次数后进入冷却（避免每个文件都先撞一次硬编）。
pub const HW_FAIL_THRESHOLD: u32 = 3;
/// 冷却时长：冷却期内强制软编，到期后自动再试硬编。
pub const HW_COOLDOWN_MS: u64 = 10 * 60 * 1000;

/// 返回 (crf, x264_preset, audio_bitrate)。
/// 画质档位只影响码率与编码速度，绝不改分辨率。
pub fn encode_params(quality_preset: &str) -> (&'static str, &'static str, &'static str) {
  match quality_preset {
    // 画质优先：稍慢但更清晰
    "quality" => ("18", "faster", "192k"),
    // 体积优先：veryfast 明显提速，画质略降仍可接受
    _ => ("23", "veryfast", "128k"),
  }
}

fn now_ms() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

#[derive(Default)]
struct HwRuntimeGate {
  consecutive_failures: u32,
  disabled_until_ms: u64,
}

fn hw_gate() -> &'static Mutex<HwRuntimeGate> {
  static GATE: OnceLock<Mutex<HwRuntimeGate>> = OnceLock::new();
  GATE.get_or_init(|| Mutex::new(HwRuntimeGate::default()))
}

fn hw_runtime_allowed() -> bool {
  let Ok(mut g) = hw_gate().lock() else {
    return true;
  };
  if g.disabled_until_ms == 0 {
    return true;
  }
  let now = now_ms();
  if now >= g.disabled_until_ms {
    g.disabled_until_ms = 0;
    g.consecutive_failures = 0;
    return true;
  }
  false
}

/// 硬编成功：清零连续失败计数。
pub fn note_hw_encode_success() {
  if let Ok(mut g) = hw_gate().lock() {
    g.consecutive_failures = 0;
  }
}

/// 硬编失败：累计连续失败；达阈值后冷却一段时间再试。
pub fn note_hw_encode_failed() {
  if let Ok(mut g) = hw_gate().lock() {
    g.consecutive_failures = g.consecutive_failures.saturating_add(1);
    if g.consecutive_failures >= HW_FAIL_THRESHOLD {
      g.disabled_until_ms = now_ms().saturating_add(HW_COOLDOWN_MS);
      g.consecutive_failures = 0;
    }
  }
}

/// 兼容旧调用名。
pub fn mark_hw_encoder_failed() {
  note_hw_encode_failed();
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderStatus {
  /// 本机探测到的硬编短名，如 VT；无硬编则为 null
  pub available_hw: Option<String>,
  /// hardware | software
  pub mode: String,
  /// 展示文案，如「当前：硬编 (VT)」
  pub mode_label: String,
  pub cooldown_remaining_sec: u64,
  pub consecutive_hw_failures: u32,
  pub hw_fail_threshold: u32,
}

pub fn encoder_status(ffmpeg: &Path) -> EncoderStatus {
  let available = preferred_hw_encoder_listed(ffmpeg).map(|k| k.short_label().to_string());
  let (cooldown_remaining_sec, consecutive_hw_failures) = {
    let Ok(g) = hw_gate().lock() else {
      return EncoderStatus {
        available_hw: available.clone(),
        mode: if available.is_some() {
          "hardware".into()
        } else {
          "software".into()
        },
        mode_label: if let Some(h) = available.as_deref() {
          format!("当前：硬编 ({h})")
        } else {
          "当前：软编 (x264)".into()
        },
        cooldown_remaining_sec: 0,
        consecutive_hw_failures: 0,
        hw_fail_threshold: HW_FAIL_THRESHOLD,
      };
    };
    let now = now_ms();
    let remaining = if g.disabled_until_ms > now {
      (g.disabled_until_ms - now) / 1000
    } else {
      0
    };
    (remaining, g.consecutive_failures)
  };

  let using_hw = available.is_some() && hw_runtime_allowed();
  let mode = if using_hw { "hardware" } else { "software" };
  let mode_label = if using_hw {
    format!(
      "当前：硬编 ({})",
      available.as_deref().unwrap_or("HW")
    )
  } else if cooldown_remaining_sec > 0 {
    format!("当前：软编 (x264)，硬编冷却约 {cooldown_remaining_sec}s")
  } else {
    "当前：软编 (x264)".into()
  };

  EncoderStatus {
    available_hw: available,
    mode: mode.into(),
    mode_label,
    cooldown_remaining_sec,
    consecutive_hw_failures,
    hw_fail_threshold: HW_FAIL_THRESHOLD,
  }
}

/// 视频编码策略：硬编优先，失败由调用方回退软编。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoEncoderKind {
  X264,
  VideoToolbox,
  /// Windows NVIDIA（macOS 构建中不会选中，保留跨平台分支）
  #[allow(dead_code)]
  Nvenc,
  #[allow(dead_code)]
  Qsv,
  #[allow(dead_code)]
  Amf,
}

impl VideoEncoderKind {
  pub fn ffmpeg_name(self) -> &'static str {
    match self {
      Self::X264 => "libx264",
      Self::VideoToolbox => "h264_videotoolbox",
      Self::Nvenc => "h264_nvenc",
      Self::Qsv => "h264_qsv",
      Self::Amf => "h264_amf",
    }
  }

  /// 任务 meta / UI 用短标签
  pub fn short_label(self) -> &'static str {
    match self {
      Self::X264 => "x264",
      Self::VideoToolbox => "VT",
      Self::Nvenc => "NVENC",
      Self::Qsv => "QSV",
      Self::Amf => "AMF",
    }
  }

  pub fn is_hardware(self) -> bool {
    !matches!(self, Self::X264)
  }
}

fn platform_hw_candidates() -> &'static [VideoEncoderKind] {
  #[cfg(target_os = "macos")]
  {
    &[VideoEncoderKind::VideoToolbox]
  }
  #[cfg(target_os = "windows")]
  {
    &[
      VideoEncoderKind::Nvenc,
      VideoEncoderKind::Qsv,
      VideoEncoderKind::Amf,
    ]
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    &[]
  }
}

fn ffmpeg_lists_encoder(ffmpeg: &Path, name: &str) -> bool {
  let mut command = Command::new(ffmpeg);
  configure_subprocess(&mut command);
  let output = command
    .args(["-hide_banner", "-encoders"])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .output();
  match output {
    Ok(out) if out.status.success() => {
      String::from_utf8_lossy(&out.stdout).lines().any(|line| {
        line.split_whitespace()
          .nth(1)
          .is_some_and(|token| token == name)
      })
    }
    _ => false,
  }
}

/// 探测本机列出的硬编（进程内缓存一次，不含运行时冷却）。
fn preferred_hw_encoder_listed(ffmpeg: &Path) -> Option<VideoEncoderKind> {
  static CACHED: OnceLock<Option<VideoEncoderKind>> = OnceLock::new();
  *CACHED.get_or_init(|| {
    for kind in platform_hw_candidates() {
      if ffmpeg_lists_encoder(ffmpeg, kind.ffmpeg_name()) {
        return Some(*kind);
      }
    }
    None
  })
}

/// 当前可用硬编：本机支持且未处于冷却期。
pub fn preferred_hw_encoder(ffmpeg: &Path) -> Option<VideoEncoderKind> {
  if !hw_runtime_allowed() {
    return None;
  }
  preferred_hw_encoder_listed(ffmpeg)
}

/// 编码尝试顺序：硬编（若有）→ libx264。
pub fn encoder_fallback_chain(ffmpeg: &Path) -> Vec<VideoEncoderKind> {
  let mut chain = Vec::with_capacity(2);
  if let Some(hw) = preferred_hw_encoder(ffmpeg) {
    chain.push(hw);
  }
  chain.push(VideoEncoderKind::X264);
  chain
}

fn hw_bitrate_bps(width: u32, height: u32, quality_preset: &str) -> u64 {
  let pixels = (width as u64).saturating_mul(height as u64).max(1);
  let base_1080 = match quality_preset {
    "quality" => 7_000_000_u64,
    _ => 3_500_000_u64,
  };
  let br = base_1080.saturating_mul(pixels) / (1920 * 1080);
  br.clamp(800_000, 20_000_000)
}

/// 向命令追加视频编码参数（不含音频）。
pub fn append_video_encode_args(
  cmd: &mut Command,
  encoder: VideoEncoderKind,
  quality_preset: &str,
  width: u32,
  height: u32,
) {
  match encoder {
    VideoEncoderKind::X264 => {
      let (crf, x264_preset, _) = encode_params(quality_preset);
      cmd.args([
        "-c:v",
        "libx264",
        "-crf",
        crf,
        "-preset",
        x264_preset,
        "-pix_fmt",
        "yuv420p",
      ]);
    }
    VideoEncoderKind::VideoToolbox => {
      let br = hw_bitrate_bps(width, height, quality_preset).to_string();
      // prio_speed：体积优先全力抢速度；画质优先仍关 realtime，避免实时档伤画质
      let prio_speed = if quality_preset == "quality" {
        "0"
      } else {
        "1"
      };
      cmd.args([
        "-c:v",
        "h264_videotoolbox",
        "-b:v",
        &br,
        "-realtime",
        "0",
        "-prio_speed",
        prio_speed,
        "-pix_fmt",
        "yuv420p",
      ]);
    }
    VideoEncoderKind::Nvenc => {
      // cq 近似 CRF：体积优先偏大、画质优先偏小；p1/p4 偏速度
      let (cq, preset) = match quality_preset {
        "quality" => ("19", "p4"),
        _ => ("28", "p1"),
      };
      cmd.args([
        "-c:v",
        "h264_nvenc",
        "-preset",
        preset,
        "-rc",
        "vbr",
        "-cq",
        cq,
        "-b:v",
        "0",
        "-pix_fmt",
        "yuv420p",
      ]);
    }
    VideoEncoderKind::Qsv => {
      let global_quality = match quality_preset {
        "quality" => "22",
        _ => "28",
      };
      cmd.args([
        "-c:v",
        "h264_qsv",
        "-global_quality",
        global_quality,
        "-look_ahead",
        "1",
        "-pix_fmt",
        "nv12",
      ]);
    }
    VideoEncoderKind::Amf => {
      let qp = match quality_preset {
        "quality" => "18",
        _ => "26",
      };
      cmd.args([
        "-c:v",
        "h264_amf",
        "-rc",
        "cqp",
        "-qp_i",
        qp,
        "-qp_p",
        qp,
        "-pix_fmt",
        "yuv420p",
      ]);
    }
  }
}

fn aac_encoder_name() -> &'static str {
  // macOS AudioToolbox AAC 通常比原生 aac 软编更快
  #[cfg(target_os = "macos")]
  {
    "aac_at"
  }
  #[cfg(not(target_os = "macos"))]
  {
    "aac"
  }
}

pub fn append_audio_aac_args(cmd: &mut Command, quality_preset: &str) {
  let (_, _, audio_bitrate) = encode_params(quality_preset);
  cmd.args(["-c:a", aac_encoder_name(), "-b:a", audio_bitrate]);
}

/// 统一音频参数（合成时视频 copy、仅转音频）。
pub fn append_audio_aac_unified_args(cmd: &mut Command, quality_preset: &str) {
  let (_, _, audio_bitrate) = encode_params(quality_preset);
  cmd.args([
    "-c:a",
    aac_encoder_name(),
    "-b:a",
    audio_bitrate,
    "-ar",
    "48000",
    "-ac",
    "2",
  ]);
}

#[cfg(test)]
mod tests {
  use super::*;

  fn reset_gate() {
    if let Ok(mut g) = hw_gate().lock() {
      *g = HwRuntimeGate::default();
    }
  }

  #[test]
  fn hw_gate_cools_down_after_threshold_failures() {
    reset_gate();
    assert!(hw_runtime_allowed());
    for _ in 0..(HW_FAIL_THRESHOLD - 1) {
      note_hw_encode_failed();
      assert!(hw_runtime_allowed());
    }
    note_hw_encode_failed();
    assert!(!hw_runtime_allowed());
    // 人为把冷却截止改到过去，应自动恢复
    if let Ok(mut g) = hw_gate().lock() {
      g.disabled_until_ms = 1;
    }
    assert!(hw_runtime_allowed());
    reset_gate();
  }

  #[test]
  fn hw_success_resets_consecutive_failures() {
    reset_gate();
    note_hw_encode_failed();
    note_hw_encode_failed();
    note_hw_encode_success();
    if let Ok(g) = hw_gate().lock() {
      assert_eq!(g.consecutive_failures, 0);
    }
    reset_gate();
  }
}
