# 刚刚好影工

跨平台桌面视频工具（Windows / macOS）。支持手动压制/合成、目录监控自动压制与任务列表；压制在**不改变分辨率**的前提下压缩体积，批量输出文件名自动加 `_batch` 后缀。

> 当前阶段：Vue + **Tauri 2** + **FFmpeg 真实压缩**。输出文件写入所选目录，命名为 `原名_batch.扩展名`，分辨率不变。

---

## 功能概览

| 能力 | 说明 |
| --- | --- |
| 压制合成 | 分别压制多个视频，或按顺序合并成片（可拖拽排序） |
| 自动压制 | 监控目录下的剧目文件夹，定时扫描并压制到 `影工输出/` |
| 任务列表 | 进度、筛选、取消、打开输出目录；完成后系统通知 |
| 系统设置 | 并行数、扫描间隔、画质档位、启动自动扫描（改完即存） |
| 批量上传 | 拖拽或选择多个视频（mp4 / mov / mkv 等） |
| 输出目录 | 桌面端使用系统原生目录选择器 |
| 命名规则 | `demo.mp4` → `demo_batch.mp4` |
| 默认档位 | **体积优先**（CRF 23）；可切换为画质优先（CRF 18），分辨率均不变 |
| 界面语言 | 中文 |

---

## 技术栈

- **界面**：Vue 3 + TypeScript + Vite
- **桌面壳**：Tauri 2
- **压缩引擎**：内置 FFmpeg sidecar（`libx264`，不缩放分辨率）；安装包自带，用户无需再装 FFmpeg

---

## 目录结构

```text
video-compressor/
├── src/                 # Vue 前端
├── src-tauri/           # Tauri 桌面壳（Rust）
├── public/              # 静态资源
├── dist/                # 前端构建产物
├── package.json
└── README.md
```

---

## 开发环境

### 前置要求

- Node.js 20+（推荐 LTS）
- npm 10+
- Rust（桌面开发/打包需要）：https://rustup.rs
- macOS：Xcode Command Line Tools（`xcode-select --install`）
- Windows：C++ Build Tools + WebView2

### 安装

```bash
cd video-compressor
npm install
# 若尚未安装 Rust：
# curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"   # macOS / Linux
```

### 启动（推荐 Makefile）

```bash
make install   # 首次：安装依赖
make dev       # 打开桌面调试窗口
make build     # 按当前系统打包（Mac→dmg，Windows→exe）
make build-mac # 仅打 macOS 包
make build-win # 仅打 Windows 包（需在 Windows 上执行）
make help      # 查看全部命令
```

也可以直接用 npm：

```bash
npm run tauri:dev    # 桌面调试
npm run tauri:build  # 打包（含 FFmpeg）
npm run dev          # 仅网页 http://127.0.0.1:5188/
```
---

## 打包成桌面安装包（Windows / macOS）

本项目使用 **Tauri 2** 打包：

- **Windows**：`.exe` 安装包（NSIS）或 MSI
- **macOS**：`.dmg` / `.app`

### 1. 安装系统依赖

#### macOS

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version
```

#### Windows

1. 安装 [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（勾选「使用 C++ 的桌面开发」）
2. 安装 [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)（较新 Win10/11 一般已自带）
3. 安装 Rust：https://rustup.rs ，完成后重新打开终端：

```powershell
rustc --version
```

### 2. 没有 Windows 电脑？用 GitHub Actions 云端打包（推荐）

本机 Mac **打不出**可用的 Windows 安装包。已配置工作流：`.github/workflows/build.yml`。

1. 把项目推到 GitHub（新建一个仓库即可）
2. 打开仓库 → **Actions** → **Build Installers** → **Run workflow**
3. 等跑完，下载产物：
   - `kuaiya-windows` → `.exe` 安装包
   - `kuaiya-macos` → `.dmg` / `.app`

打 tag 也会自动触发：

```bash
git tag v0.1.0
git push origin v0.1.0
```

### 3. 本机正式打包（含内置 FFmpeg）

```bash
# macOS
make build-mac

# Windows（必须在 Windows 上）
make build-win
```

或：

```bash
npm install
npm run tauri:build
```

`tauri:build` 会自动执行 `prepare:ffmpeg`：从可移植静态包复制 `ffmpeg` / `ffprobe` 到 `src-tauri/binaries/`，再由 Tauri `externalBin` 打进安装包。

用户安装后**不需要**再安装 FFmpeg。

> 不要拷贝 Homebrew / apt 的 ffmpeg 去打包（依赖本机动态库，换机器会坏）。请始终用 `npm run prepare:ffmpeg`。

#### 产物位置

| 平台 | 典型产物路径 |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/dmg/*.dmg` |
| macOS | `src-tauri/target/release/bundle/macos/*.app` |
| Windows | `src-tauri/target/release/bundle/nsis/*-setup.exe` |
| Windows | `src-tauri/target/release/bundle/msi/*.msi` |

也可直接打开：

```text
src-tauri/target/release/bundle/
```

### 4. 打包注意点

1. **无 Windows 电脑**：用上面的 GitHub Actions，不要在 Mac 上硬交叉编译
2. **macOS 对外分发**：建议签名与公证；本地自用可直接打开 `.app` / `.dmg`
3. **Windows SmartScreen**：未签名安装包可能被拦截，自用可选「仍要运行」
4. **安装包体积**：内置 ffmpeg + ffprobe 大约会增加数十 MB
5. **二进制默认不进 Git**：体积大；CI/本机打包前靠 `prepare:ffmpeg` 生成

### 5. 配置打包格式（可选）

编辑 `src-tauri/tauri.conf.json`：

```json
{
  "bundle": {
    "active": true,
    "targets": "all"
  }
}
```

- `targets: "all"`：按当前系统生成可用安装包
- 只要 Windows 安装包：`["nsis"]`
- 只要 macOS 安装包：`["dmg"]`

---

## 版本与命名约定

- 输出文件：`{原文件名去掉扩展名}_batch.{扩展名}`
- 默认压缩策略：画质优先（分辨率不变）
- UI：中文

---

## 开发路线

1. ~~交互稿：批量上传、选择输出目录、`_batch` 命名~~
2. ~~接入 Tauri 桌面壳与系统目录/文件选择~~
3. ~~接入 FFmpeg，实现真实压缩（保持分辨率）~~
4. ~~打包内置 FFmpeg sidecar~~；待完善：失败重试与签名

### FFmpeg 依赖

- **正式打包 / 推荐开发**：`npm run prepare:ffmpeg`（内置可移植二进制）
- **可选**：本机 `brew install ffmpeg` 仅作开发回退

---

## 许可证

Private / 未开源声明前仅供内部使用。
# video-compressor
