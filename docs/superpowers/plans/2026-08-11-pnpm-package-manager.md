# pnpm Package Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make pnpm the sole dependency installer while retaining npm/pnpm script compatibility and a working `make build` entry point.

**Architecture:** Configure pnpm’s build-script allowlist and package-manager version in repository metadata. Replace npm-specific invocations in package scripts and Make targets with pnpm equivalents, then remove the competing npm lockfile.

**Tech Stack:** pnpm 11.7.0, Corepack, Make, Tauri CLI.

## Global Constraints

- `pnpm-lock.yaml` is the sole committed dependency lockfile.
- Permit build scripts only for `ffmpeg-static`, `@ffprobe-installer/darwin-arm64`, and `esbuild`.
- `make build`, `make build-mac`, and `make build-win` use pnpm.

---

### Task 1: Configure pnpm and update Make targets

**Files:**
- Modify: `package.json`
- Modify: `pnpm-workspace.yaml`
- Modify: `Makefile`
- Delete: `package-lock.json`

- [ ] Set `packageManager` to `pnpm@11.7.0`.
- [ ] Replace npm-specific package-script chaining with package-manager-neutral commands.
- [ ] Set all three required `allowBuilds` entries to `true`.
- [ ] Change Make targets to invoke `pnpm install`, `pnpm run prepare:ffmpeg`, `pnpm run tauri:dev`, `pnpm run dev`, and `pnpm exec tauri build`.
- [ ] Remove `package-lock.json`.
- [ ] Verify with `pnpm install`, `pnpm run prepare:ffmpeg`, `pnpm run build`, and `make -n build`.
