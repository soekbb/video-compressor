export const TWO_DAYS_MS = 2 * 24 * 60 * 60 * 1000

export function isDramaFolderFresh(
  createdAtMs: number,
  nowMs: number,
  maxAgeMs: number = TWO_DAYS_MS,
): boolean {
  if (!Number.isFinite(createdAtMs) || createdAtMs <= 0) return false
  return nowMs - createdAtMs <= maxAgeMs
}

export function shouldMonitorOldDrama(createdAtMs: number, nowMs: number): boolean {
  return isDramaFolderFresh(createdAtMs, nowMs)
}

export function pendingVideosForDrama<T extends { name: string }>(
  folder: { videos: T[] },
  record: { videoNames?: string[] } | null | undefined,
): T[] {
  const done = new Set(record?.videoNames ?? [])
  return folder.videos.filter((v) => !done.has(v.name))
}

export function seedVideoNames(folder: { videos: { name: string }[] }): string[] {
  return folder.videos.map((v) => v.name)
}

/**
 * Videos to compress for a drama this scan pass.
 * - null: legacy record without videoNames, folder ≤2d → seed only, do not compress
 * - []: skip (too old including legacy, or nothing pending)
 * - T[]: enqueue these
 */
export function videosToProcess<T extends { name: string }>(
  folder: { videos: T[]; createdAtMs: number },
  record: { videoNames?: string[] } | null | undefined,
  nowMs: number = Date.now(),
): T[] | null {
  if (!record) {
    return folder.videos
  }
  if (!shouldMonitorOldDrama(folder.createdAtMs, nowMs)) {
    return []
  }
  // Legacy done row without videoNames: seed only when still within 2 days
  if (!record.videoNames) {
    return null
  }
  return pendingVideosForDrama(folder, record)
}
