use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// 返回 (crf, x264_preset, audio_bitrate)。
/// 画质档位只影响码率与编码速度，绝不改分辨率。
pub fn encode_params(quality_preset: &str) -> (&'static str, &'static str, &'static str) {
  match quality_preset {
    "quality" => ("18", "medium", "192k"),
    _ => ("23", "faster", "128k"),
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
  let output = Command::new(ffmpeg)
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

/// 探测本机可用的硬编（进程内缓存一次）。
pub fn preferred_hw_encoder(ffmpeg: &Path) -> Option<VideoEncoderKind> {
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
      cmd.args([
        "-c:v",
        "h264_videotoolbox",
        "-b:v",
        &br,
        "-realtime",
        "0",
        "-pix_fmt",
        "yuv420p",
      ]);
    }
    VideoEncoderKind::Nvenc => {
      // cq 近似 CRF：体积优先偏大、画质优先偏小
      let cq = match quality_preset {
        "quality" => "19",
        _ => "28",
      };
      cmd.args([
        "-c:v",
        "h264_nvenc",
        "-preset",
        "p4",
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

pub fn append_audio_aac_args(cmd: &mut Command, quality_preset: &str) {
  let (_, _, audio_bitrate) = encode_params(quality_preset);
  cmd.args(["-c:a", "aac", "-b:a", audio_bitrate]);
}

/// 统一音频参数（合成时视频 copy、仅转音频）。
pub fn append_audio_aac_unified_args(cmd: &mut Command, quality_preset: &str) {
  let (_, _, audio_bitrate) = encode_params(quality_preset);
  cmd.args([
    "-c:a",
    "aac",
    "-b:a",
    audio_bitrate,
    "-ar",
    "48000",
    "-ac",
    "2",
  ]);
}
