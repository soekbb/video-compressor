//! 把 FFmpeg 的 stderr 归类成用户能看懂的中文原因，并保留末尾几行原始输出。
//!
//! 背景：压缩/合成失败时只把最后一行 stderr 抛给用户，会出现「压缩失败（libx264）：
//! Nothing was written into output file…」这种看起来像编码器故障、实际是源文件损坏的提示。
//! 这里统一做两件事：
//!   1. 按已知的 FFmpeg 输出特征给出中文原因（判定顺序敏感，先具体后笼统）；
//!   2. 顺带保留末尾几行原始输出，便于用户反馈问题、便于排查。

/// 保留的原始 stderr 行数。
pub const TAIL_LINES: usize = 3;

/// 源文件缺少 moov（元数据在文件尾部但没下载完）。
pub const MOOV_MISSING: &str =
  "视频不完整：缺少 moov（索引信息），无法读取画面。请重新下载或重新导出该文件";

/// 媒体数据被截断。
pub const TRUNCATED: &str = "文件数据不完整：媒体内容被截断，多半是下载未完成，请重新下载该文件";

/// 画面/声音数据损坏，解码不出来。
pub const DECODE_CORRUPTED: &str =
  "画面数据已损坏，无法解码：多半是下载不完整或源文件本身损坏，请重新下载该文件";

/// 一帧都没写进输出，通常等于源文件里没有可解码的画面。
pub const NO_VIDEO_PACKETS: &str =
  "源文件里没有可解码的画面数据：多半是文件只有元数据、没有正片内容（下载不完整），请重新下载该文件";

/// 读不出画面格式。
pub const FORMAT_UNKNOWN: &str =
  "读不出画面格式：文件里没有可解码的视频数据，多半是下载不完整，请重新下载该文件";

/// 通用「数据处理失败」。
pub const INVALID_DATA: &str =
  "文件数据无法解析：多半是下载不完整或文件已损坏，请重新下载该文件";

/// 磁盘空间不足。
pub const DISK_FULL: &str = "磁盘空间不足，无法写出文件，请清理磁盘后重试";

/// 权限或占用问题。
pub const ACCESS_DENIED: &str = "没有读写权限或文件被其它程序占用，请关闭占用后重试";

/// 输入文件不存在。
pub const MISSING_FILE: &str = "找不到输入文件（可能已被移动或删除）";

/// 内置 FFmpeg 缺少所需的编解码器。
pub const CODEC_MISSING: &str = "内置 FFmpeg 缺少所需的编解码器，请更新应用";

/// 判定规则：(命中任一特征串, 中文原因)。特征串一律用小写比较。
///
/// 顺序即优先级：文件级的不完整/损坏特征要排在「没写出文件」这类兜底信息前面，
/// 否则真正的根因会被兜底信息盖住。
const RULES: &[(&[&str], &str)] = &[
  (&["moov atom not found"], MOOV_MISSING),
  (&["partial file", "truncated", "unexpected end of file"], TRUNCATED),
  (
    &[
      "error while decoding stream",
      "invalid nal unit size",
      "error splitting the input into nal units",
      "no frame!",
      "channel element",
      "is not allocated",
      "packet corrupt",
      "corrupted",
      "decode_slice_header error",
    ],
    DECODE_CORRUPTED,
  ),
  (
    &["nothing was written into output file", "received no packets"],
    NO_VIDEO_PACKETS,
  ),
  (
    &[
      "cannot determine format of input stream",
      "could not find codec parameters",
      "unspecified pixel format",
    ],
    FORMAT_UNKNOWN,
  ),
  (&["no space left on device", "disk full"], DISK_FULL),
  (
    &["permission denied", "being used by another process", "text file busy"],
    ACCESS_DENIED,
  ),
  (&["no such file or directory"], MISSING_FILE),
  (
    &["unknown encoder", "unknown decoder", "encoder not found", "decoder not found"],
    CODEC_MISSING,
  ),
  (&["invalid data found when processing input"], INVALID_DATA),
];

/// 按 FFmpeg 输出判断中文原因；识别不出时返回 None。
pub fn classify(stderr: &str) -> Option<&'static str> {
  let haystack = stderr.to_ascii_lowercase();
  RULES
    .iter()
    .find(|(needles, _)| needles.iter().any(|needle| haystack.contains(needle)))
    .map(|(_, reason)| *reason)
}

/// 取 stderr 末尾若干条有用的行（去掉空行、FFmpeg 的重复计数提示与完全重复的行）。
pub fn stderr_tail(stderr: &str, limit: usize) -> Vec<String> {
  let mut kept: Vec<String> = Vec::new();
  for line in stderr.lines().rev() {
    let line = line.trim();
    if line.is_empty() || line.starts_with("Last message repeated") {
      continue;
    }
    if kept.iter().any(|existing| existing == line) {
      continue;
    }
    kept.push(line.to_string());
    if kept.len() >= limit {
      break;
    }
  }
  kept.reverse();
  kept
}

/// 生成给用户看的失败详情：中文原因（能判断时）+ 末尾原始输出。
///
/// 识别不出原因时退化成末尾几行原文，不再只给一行。
pub fn ffmpeg_failure_detail(stderr: &str, fallback: &str) -> String {
  let tail = stderr_tail(stderr, TAIL_LINES);
  let mut out = String::new();

  if let Some(reason) = classify(stderr) {
    out.push_str(reason);
  }
  if !tail.is_empty() {
    if !out.is_empty() {
      out.push_str("\n\n");
    }
    out.push_str(&format!("FFmpeg 原始输出（末尾 {} 行）：", tail.len()));
    for line in &tail {
      out.push('\n');
      out.push_str(line);
    }
  }

  if out.is_empty() {
    fallback.to_string()
  } else {
    out
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn classifies_missing_packets_as_no_video_data() {
    let stderr = "[out#0/mp4 @ 0x23333178300] Nothing was written into output file, \
                  because at least one of its streams received no packets.\n";
    assert_eq!(classify(stderr), Some(NO_VIDEO_PACKETS));
    let detail = ffmpeg_failure_detail(stderr, "压缩失败");
    assert!(detail.starts_with("源文件里没有可解码的画面数据"), "{detail}");
    assert!(detail.contains("Nothing was written into output file"), "{detail}");
  }

  #[test]
  fn decode_corruption_wins_over_the_generic_trailer_message() {
    // 真实损坏文件会长这样：先刷一堆解码错误，最后才是封装器的兜底信息。
    let stderr = "\
[h264 @ 0x1] Invalid NAL unit size (0 > 376).
[aac @ 0x2] channel element 0.0 is not allocated
Error while decoding stream #0:0: Invalid data found when processing input
[out#0/mp4 @ 0x3] Nothing was written into output file, because at least one of its streams received no packets.
";
    assert_eq!(classify(stderr), Some(DECODE_CORRUPTED));
  }

  #[test]
  fn classifies_incomplete_and_unreadable_files() {
    assert_eq!(
      classify("moov atom not found\nInvalid data found when processing input\n"),
      Some(MOOV_MISSING)
    );
    assert_eq!(
      classify("stream 0, offset 0x1fa8: partial file\n"),
      Some(TRUNCATED)
    );
    assert_eq!(
      classify("Cannot determine format of input stream 0:0 after EOF\n"),
      Some(FORMAT_UNKNOWN)
    );
    assert_eq!(
      classify("Error initializing output stream 0:0 -- Error while opening encoder\nunknown encoder 'libx264'\n"),
      Some(CODEC_MISSING)
    );
    assert_eq!(classify("nothing useful here"), None);
  }

  #[test]
  fn tail_keeps_last_distinct_useful_lines() {
    let stderr = "\
first line
Error while decoding stream #0:0: Invalid data found when processing input
Error while decoding stream #0:0: Invalid data found when processing input

    Last message repeated 9 times
last line
";
    // 去重 + 跳过「Last message repeated N times」与空行
    assert_eq!(
      stderr_tail(stderr, 3),
      vec![
        "first line".to_string(),
        "Error while decoding stream #0:0: Invalid data found when processing input".to_string(),
        "last line".to_string(),
      ]
    );
    assert_eq!(
      stderr_tail(stderr, 2),
      vec![
        "Error while decoding stream #0:0: Invalid data found when processing input".to_string(),
        "last line".to_string(),
      ]
    );
    assert_eq!(stderr_tail(stderr, 1), vec!["last line".to_string()]);
    assert!(stderr_tail("\n\n  \n", 3).is_empty());
  }

  #[test]
  fn unknown_error_falls_back_to_raw_tail() {
    let detail = ffmpeg_failure_detail("some weird failure\nplain tail\n", "压缩失败");
    assert!(!detail.contains("压缩失败"));
    assert!(detail.contains("some weird failure"));
    assert!(detail.contains("plain tail"));
    assert_eq!(ffmpeg_failure_detail("", "压缩失败"), "压缩失败");
  }
}
