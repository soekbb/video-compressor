export type FileStatus = 'pending' | 'running' | 'done' | 'error'

export type QueueItem = {
  id: string
  name: string
  size: number
  path: string
  status: FileStatus
  progress: number
  outputName: string
  outputPath?: string
  outputSize?: number
  error?: string
}

export type AppPage = 'workbench' | 'auto' | 'tasks' | 'settings'

/** size=体积优先，quality=画质优先 */
export type QualityPreset = 'size' | 'quality'

export type AppSettings = {
  concurrency: number
  scanIntervalMinutes: number
  qualityPreset: QualityPreset
  /** 打开软件时是否自动开启自动压制扫描 */
  autoScanOnLaunch: boolean
}

export type SystemInfo = {
  os: string
  arch: string
  cpuBrand?: string | null
  cpuCores: number
  totalMemoryBytes?: number | null
}

export type DramaFolder = {
  name: string
  path: string
  videoCount: number
  /** Unix ms folder birth/ctime/mtime from Rust; 0 if unknown */
  createdAtMs: number
  videos: { name: string; path: string; size: number }[]
}

export type AutoVideoFailure = {
  name: string
  reason: string
  at: string
}

export type AutoRecord = {
  path: string
  name: string
  completedAt: string
  videoCount: number
  videoNames?: string[]
  failures?: AutoVideoFailure[]
}
