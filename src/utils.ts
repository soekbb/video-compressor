export function formatSize(bytes: number) {
  if (!bytes) return '大小未知'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export function toBatchName(name: string) {
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return `${name}_batch`
  return `${name.slice(0, dot)}_batch${name.slice(dot)}`
}

/** 合成输出文件名未以 .mp4 结尾时自动补全（大小写不敏感）。 */
export function ensureMp4Extension(name: string) {
  const trimmed = name.trim()
  if (!trimmed) return trimmed
  if (/\.mp4$/i.test(trimmed)) return trimmed
  return `${trimmed}.mp4`
}

export function toOutputPath(outputDir: string, outputName: string) {
  const separator = outputDir.includes('\\') && !outputDir.includes('/') ? '\\' : '/'
  const cleanName = outputName.replace(/^[\\/]+/, '')
  return /[\\/]$/.test(outputDir) ? `${outputDir}${cleanName}` : `${outputDir}${separator}${cleanName}`
}

type OutputPathPlatform = 'linux' | 'macos' | 'windows'

function currentOutputPathPlatform(): OutputPathPlatform {
  if (typeof navigator === 'undefined') return 'linux'
  const identity = `${navigator.platform} ${navigator.userAgent}`
  if (/win/i.test(identity)) return 'windows'
  if (/mac/i.test(identity)) return 'macos'
  return 'linux'
}

function outputPathKey(path: string, platform: OutputPathPlatform) {
  const normalized =
    platform === 'windows'
      ? path.replaceAll('\\', '/').replace(/\/+/g, '/')
      : path.replace(/\/+/g, '/')
  return platform === 'linux' ? normalized : normalized.toLowerCase()
}

export function assertUniqueOutputPaths(
  paths: string[],
  platform: OutputPathPlatform = currentOutputPathPlatform(),
) {
  const seen = new Map<string, string>()
  for (const path of paths) {
    const key = outputPathKey(path, platform)
    const previous = seen.get(key)
    if (previous) {
      throw new Error(`输出路径冲突：${previous} 与 ${path}`)
    }
    seen.set(key, path)
  }
}

export type BatchCompressionItem = {
  inputPath: string
  outputPath: string
}

type BatchCompressionFailure = {
  inputPath: string
  message: string
}

export type BatchCompressionItemResult =
  | { status: 'completed'; encoder?: string }
  | { status: 'skipped' }
  | { status: 'cancelled' }
  | { status: 'failed'; failure: BatchCompressionFailure }

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message || '压制失败'
  const message = String(error)
  return message || '压制失败'
}

export async function runBatchCompressionItem(
  item: BatchCompressionItem,
  operations: {
    isOutputValid: (outputPath: string) => Promise<boolean>
    compress: () => Promise<unknown>
    isCancelled: () => boolean
  },
): Promise<BatchCompressionItemResult> {
  if (operations.isCancelled()) return { status: 'cancelled' }

  try {
    if (await operations.isOutputValid(item.outputPath)) return { status: 'skipped' }
  } catch (error) {
    if (operations.isCancelled()) return { status: 'cancelled' }
    return {
      status: 'failed',
      failure: { inputPath: item.inputPath, message: errorMessage(error) },
    }
  }

  for (let attempt = 0; attempt < 2; attempt += 1) {
    if (operations.isCancelled()) return { status: 'cancelled' }
    try {
      const compressResult = await operations.compress()
      if (operations.isCancelled()) return { status: 'cancelled' }
      const encoder =
        compressResult &&
        typeof compressResult === 'object' &&
        'encoder' in compressResult &&
        typeof (compressResult as { encoder?: unknown }).encoder === 'string'
          ? String((compressResult as { encoder: string }).encoder)
          : undefined
      return { status: 'completed', encoder }
    } catch (error) {
      if (operations.isCancelled()) return { status: 'cancelled' }
      if (attempt === 1) {
        return {
          status: 'failed',
          failure: { inputPath: item.inputPath, message: errorMessage(error) },
        }
      }
    }
  }

  return { status: 'cancelled' }
}

export function makeId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

export function isVideoFileName(name: string) {
  return /\.(mp4|mov|mkv|avi|webm|m4v|wmv|flv)$/i.test(name)
}
