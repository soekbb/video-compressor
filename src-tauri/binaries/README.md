# FFmpeg sidecar 二进制

打包时会把这里的 `ffmpeg` / `ffprobe` 打进安装包，用户机器无需单独安装 FFmpeg。

## 生成方式

在项目根目录执行：

```bash
npm run prepare:ffmpeg
```

会生成类似：

```text
ffmpeg-aarch64-apple-darwin
ffprobe-aarch64-apple-darwin
```

Windows 上则为：

```text
ffmpeg-x86_64-pc-windows-msvc.exe
ffprobe-x86_64-pc-windows-msvc.exe
```

## 注意

- 请使用脚本下载的**可移植静态包**，不要直接拷贝 Homebrew / apt 的 ffmpeg（依赖本机动态库，换机器会坏）。
- 这些文件体积较大，默认不提交到 Git；打包前务必先跑 `prepare:ffmpeg`。
