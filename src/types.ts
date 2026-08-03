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

export type AppPage = 'batch' | 'merge' | 'auto' | 'tasks' | 'settings'

export type AppSettings = {
  concurrency: number
  scanIntervalMinutes: number
}

export type DramaFolder = {
  name: string
  path: string
  videoCount: number
  videos: { name: string; path: string; size: number }[]
}

export type AutoRecord = {
  path: string
  name: string
  completedAt: string
  videoCount: number
}
