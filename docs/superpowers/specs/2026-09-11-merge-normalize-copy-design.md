# 合成：只转不匹配分集后无损拼接

## 目标

分辨率已经一致、仅少数分集帧率或 timebase 不同时，只重编码这些分集，再 concat demuxer `-c copy`。避免把整部剧全部 filter 重编码。

## 范围

- **改动**：`src-tauri/src/media.rs`（策略选择、单片规范化、临时文件、回退）及同文件测试
- **不改**：前端 UI、任务状态模型、分辨率不一致时的行为、取消键语义

## 策略顺序（分辨率一致时）

1. 全部视频+音频指纹一致 → 现有 `FullCopy`
2. 视频一致、音频不一致 → 现有 `VideoCopyAudioEncode`
3. 视频指纹不完全一致 → **NormalizeThenCopy**（本功能）
4. 上述失败 → 现有 `FullReencode`（filter concat）

分辨率不一致：未勾选统一分辨率则报错；勾选了直接 `FullReencode`。不走本路径。

## 目标指纹

在全部输入的**视频指纹**中取出现次数最多的一组作为目标（宽高、codec、pix_fmt、profile、r_frame_rate、time_base）。次数相同则取下标更小的（更靠前的分集）。该条分集的完整指纹（含音频）同时作为音频对齐参考。

和目标视频指纹一致的分集用原文件；否则规范化到临时文件后再参与拼接。

## 单片规范化

对每个需要转的分集：

- 滤镜：`setpts=PTS-STARTPTS,fps={目标r_frame_rate}`；分辨率已与目标相同，**不** `scale`
- 视频：软编 libx264（与当前画质档位相同的 crf/preset），`pix_fmt` 对齐目标，`-bf 0`（便于与无 B 帧源片 copy）
- MP4 timescale：目标 `time_base` 为 `1/N` 时加 `-video_track_timescale N`；无法解析则本路径失败，回退全量重编码
- 音频：与目标音频指纹一致则 `-c:a copy`，否则按现有统一 AAC 参数重编码
- 输出到临时文件 `kuaiya-norm-{任务id}-{index}.mp4`
- 转完用现有 `probe_stream_fingerprint` 核对：视频指纹必须与目标 `can_copy_video`；否则本路径失败

规范化过程使用硬编不可靠（timescale/B 帧难对齐），此路径只用 x264。

## 拼接与清理

规范化列表（原路径或临时路径）写入 concat list，再走现有 demuxer：

- 音频全部一致：`-c copy`
- 否则：`-c:v copy` + 统一 AAC

成功、失败、取消都删除本次任务产生的临时文件。取消仍检查现有 `cancel_key`，尽快停当前 FFmpeg。

## 进度

规范化阶段按「待转分集时长之和」计 0–90%；copy 拼接 90–99%。无待转分集时不应进入本路径。

## 测试

- 多数派目标：4 条 25fps + 1 条 24fps（24fps 在首位）→ 目标为 25fps 那一组里下标最小者，需规范化的是下标 0
- 全部一致 → 无需规范化
- `time_base` `1/12800` → timescale `12800`；非法 timebase → 无 timescale
- 策略：视频不一致且分辨率一致时，在 FullReencode 之前选择 NormalizeThenCopy
