export type EncoderFileUse = {
  name: string
  encoder: string
}

export function fileNameFromPath(path: string) {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || path
}

export function bumpEncoderCount(counts: Record<string, number>, encoder: string) {
  const key = encoder.trim() || 'unknown'
  counts[key] = (counts[key] ?? 0) + 1
}

export function formatEncoderSummary(counts: Record<string, number> | undefined): string {
  if (!counts) return ''
  return Object.entries(counts)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([name, n]) => `${name}×${n}`)
    .join(' · ')
}

export function extractEncoder(result: unknown): string | undefined {
  if (!result || typeof result !== 'object') return undefined
  const encoder = (result as { encoder?: unknown }).encoder
  return typeof encoder === 'string' && encoder.trim() ? encoder.trim() : undefined
}
