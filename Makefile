# 刚刚好影工 · 常用命令
#   make dev        打开桌面调试
#   make build      按当前系统打包
#   make build-mac  打 macOS 安装包（.dmg / .app）
#   make build-win  打 Windows 安装包（.exe，需在 Windows 上执行）

SHELL := /bin/bash
.DEFAULT_GOAL := help

UNAME_S := $(shell uname -s)
CARGO_ENV := $(HOME)/.cargo/env

# 确保 release 产物落在项目内，而不是外部缓存目录
export CARGO_TARGET_DIR := $(CURDIR)/src-tauri/target

define with_cargo
	@if [ -f "$(CARGO_ENV)" ]; then source "$(CARGO_ENV)"; fi; \
	export CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)"; \
	$(1)
endef

.PHONY: help install prepare-ffmpeg dev web build build-mac build-win clean

help:
	@echo ""
	@echo "刚刚好影工 · Makefile"
	@echo "  make install      安装依赖"
	@echo "  make dev          打开桌面调试（Tauri + Vite）"
	@echo "  make web          仅打开网页调试 http://127.0.0.1:5188"
	@echo "  make build        按当前系统打包（自动含 FFmpeg）"
	@echo "  make build-mac    打包 macOS（.dmg / .app）"
	@echo "  make build-win    打包 Windows（.exe；无 Win 电脑请用 GitHub Actions）"
	@echo "  make prepare-ffmpeg  仅准备内置 FFmpeg 二进制"
	@echo "  make clean        清理构建产物"
	@echo ""

install:
	npm install

prepare-ffmpeg:
	npm run prepare:ffmpeg

# 桌面调试：原生窗口 + 热更新
dev:
	$(call with_cargo,npm run tauri:dev)

# 仅前端网页调试
web:
	npm run dev -- --host 127.0.0.1 --port 5188

# 按当前操作系统打包
build:
ifeq ($(UNAME_S),Darwin)
	@$(MAKE) build-mac
else ifeq ($(OS),Windows_NT)
	@$(MAKE) build-win
else
	@echo "当前系统未单独配置目标，改为打全量 bundle…"
	$(call with_cargo,npm run prepare:ffmpeg && npx tauri build)
endif

# macOS：dmg + app
build-mac:
ifeq ($(UNAME_S),Darwin)
	@echo "→ 打包 macOS（含内置 FFmpeg）…"
	$(call with_cargo,npm run prepare:ffmpeg && npx tauri build --bundles dmg,app)
	@echo ""
	@echo "产物目录：src-tauri/target/release/bundle/"
	@ls -la src-tauri/target/release/bundle/dmg 2>/dev/null || true
	@ls -la src-tauri/target/release/bundle/macos 2>/dev/null || true
else
	@echo "错误：build-mac 请在 macOS 上执行（当前是 $(UNAME_S)）"
	@exit 1
endif

# Windows：NSIS 安装包（.exe）
# 注意：需在 Windows 本机执行；Mac 上无法直接交叉打出可用 exe
build-win:
ifeq ($(OS),Windows_NT)
	@echo "→ 打包 Windows（含内置 FFmpeg）…"
	npm run prepare:ffmpeg
	npx tauri build --bundles nsis
	@echo ""
	@echo "产物目录：src-tauri/target/release/bundle/nsis/"
else
	@echo "错误：build-win 请在 Windows 上执行。"
	@echo "在 Mac 上请用：make build-mac"
	@echo "把项目拷到 Windows 后执行：make build-win 或 npm run tauri:build"
	@exit 1
endif

clean:
	rm -rf dist
	rm -rf src-tauri/target
	rm -f src-tauri/binaries/ffmpeg-* src-tauri/binaries/ffprobe-*
	@echo "已清理 dist / target / ffmpeg sidecar"
