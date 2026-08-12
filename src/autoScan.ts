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
