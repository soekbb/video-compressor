# pnpm 包管理与 Make 打包入口

## 目标

使用 pnpm 作为唯一依赖安装器，允许 npm 或 pnpm 运行项目脚本，并保证 `make build` 能完成当前平台的 Tauri 打包。

## 设计

- 保留 `pnpm-lock.yaml`，删除 `package-lock.json`。
- 在 `package.json` 固定 `packageManager` 为 `pnpm@11.7.0`。
- 在 `pnpm-workspace.yaml` 将 `ffmpeg-static`、`@ffprobe-installer/darwin-arm64` 与 `esbuild` 的 `allowBuilds` 设为 `true`，让安装脚本下载并准备必要二进制。
- 所有 package script 不再调用 `npm run`；脚本命令本身可由 `pnpm <script>` 或 `npm run <script>` 启动。
- Makefile 的安装、开发、准备 FFmpeg 与各平台打包目标改用 pnpm；`make build` 保留为当前系统的统一打包入口。

## 验证

全新依赖目录执行 `pnpm install` 后，`pnpm run prepare:ffmpeg` 能找到二进制，`make build` 能进入对应平台的 Tauri 打包流程。
