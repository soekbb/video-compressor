# 隐藏 Windows FFmpeg 控制台窗口

## 目标

Windows 桌面版执行压缩、合成、媒体探测和 FFmpeg/FFprobe 可执行性检测时，不显示控制台窗口。macOS 与 Linux 的子进程行为不变。

## 设计

在 `src-tauri/src/compress.rs` 增加一个公共辅助函数，接收可变 `std::process::Command`：

- Windows 下通过 `std::os::windows::process::CommandExt::creation_flags` 设置 `CREATE_NO_WINDOW`（`0x08000000`）。
- 非 Windows 平台实现为空操作，以便调用点无条件调用且保持跨平台编译。

压缩和合成模块中，每个 FFmpeg/FFprobe `Command` 创建后都调用此函数。这覆盖：

- FFmpeg、FFprobe 的可执行性检测；
- 视频分辨率、时长、编码器及流信息探测；
- 压缩和合成的实际 FFmpeg 进程；
- Unix 下的残留进程清理命令保持原样。

不改变命令行参数、标准输入输出重定向、取消机制或编码回退逻辑。

## 验证

Add a Windows-only compilation test for the subprocess configuration helper. Run Rust formatting and tests; on a Windows build, run a compression, a merge, and a media scan to confirm no console window appears and all tasks finish normally.
