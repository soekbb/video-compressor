import { execSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, '..')
const outDir = path.join(root, 'src-tauri', 'binaries')

const out = execSync('rustc -vV', { encoding: 'utf8' })
const triple = (process.env.TAURI_ENV_TARGET_TRIPLE || out.match(/host:\s+(\S+)/)?.[1] || '').trim()
const ext = process.platform === 'win32' ? '.exe' : ''

const needed = [
  path.join(outDir, `ffmpeg-${triple}${ext}`),
  path.join(outDir, `ffprobe-${triple}${ext}`),
]

if (needed.every((p) => fs.existsSync(p))) {
  console.log('内置 FFmpeg 已就绪，跳过准备')
  process.exit(0)
}

console.log('缺少内置 FFmpeg，开始准备…')
execSync('npm run prepare:ffmpeg', { cwd: root, stdio: 'inherit' })
