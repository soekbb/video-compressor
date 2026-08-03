import { createRequire } from 'node:module'
import { execFileSync, execSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, '..')
const outDir = path.join(root, 'src-tauri', 'binaries')

function hostTriple() {
  const fromEnv = process.env.TAURI_ENV_TARGET_TRIPLE || process.env.CARGO_BUILD_TARGET
  if (fromEnv) return fromEnv.trim()

  const out = execSync('rustc -vV', { encoding: 'utf8' })
  const match = out.match(/host:\s+(\S+)/)
  if (!match) throw new Error('无法从 rustc -vV 解析 host triple')
  return match[1]
}

function clearQuarantine(filePath) {
  if (process.platform !== 'darwin') return
  try {
    execFileSync('xattr', ['-cr', filePath])
  } catch {
    // ignore
  }
}

function sha1(filePath) {
  const hash = crypto.createHash('sha1')
  hash.update(fs.readFileSync(filePath))
  return hash.digest('hex')
}

function copyBin(source, target) {
  fs.mkdirSync(path.dirname(target), { recursive: true })

  // 内容相同则不改写，避免触发 tauri watch 重建
  if (fs.existsSync(target) && sha1(source) === sha1(target)) {
    console.log(`✓ ${path.basename(target)} (已是最新，跳过)`)
    return
  }

  fs.copyFileSync(source, target)
  fs.chmodSync(target, 0o755)
  clearQuarantine(target)
  const sizeMb = (fs.statSync(target).size / (1024 * 1024)).toFixed(1)
  console.log(`✓ ${path.basename(target)} (${sizeMb} MB)`)
}

const triple = hostTriple()
const ext = process.platform === 'win32' ? '.exe' : ''

const ffmpegSource = require('ffmpeg-static')
const ffprobeSource = require('@ffprobe-installer/ffprobe').path

if (!ffmpegSource || !fs.existsSync(ffmpegSource)) {
  throw new Error('未找到 ffmpeg-static 二进制，请先 npm install')
}
if (!ffprobeSource || !fs.existsSync(ffprobeSource)) {
  throw new Error('未找到 ffprobe 二进制，请先 npm install')
}

console.log(`准备 sidecar 二进制 → ${triple}`)
copyBin(ffmpegSource, path.join(outDir, `ffmpeg-${triple}${ext}`))
copyBin(ffprobeSource, path.join(outDir, `ffprobe-${triple}${ext}`))
console.log(`完成：${outDir}`)
