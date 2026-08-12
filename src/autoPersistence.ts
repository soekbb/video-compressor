import type { AutoRecord } from './types.ts'

/** Pure RMW merge for one successful video name (caller must serialize if concurrent). */
export function mergeAppendDramaVideoSuccess(
  records: AutoRecord[],
  args: { path: string; name: string; videoName: string; at: string },
): AutoRecord[] {
  const prev = records.find((r) => r.path === args.path)
  const names = new Set(prev?.videoNames ?? [])
  names.add(args.videoName)
  const videoNames = [...names]
  const failures = (prev?.failures ?? []).filter((f) => f.name !== args.videoName)
  const nextRecord: AutoRecord = {
    path: args.path,
    name: args.name,
    completedAt: args.at,
    videoCount: videoNames.length,
    videoNames,
    failures: failures.length ? failures : [],
  }
  const idx = records.findIndex((r) => r.path === args.path)
  return idx === -1
    ? [nextRecord, ...records]
    : records.map((item, i) => (i === idx ? nextRecord : item))
}

export async function publishPersistedDramaDone(
  records: AutoRecord[],
  record: AutoRecord,
  operations: {
    persist: (records: AutoRecord[]) => Promise<unknown>
    mirror?: (records: AutoRecord[]) => void
    onMirrorError?: (error: unknown) => void
    publish: (records: AutoRecord[]) => void
  },
) {
  if (records.some((item) => item.path === record.path)) return
  const next = [record, ...records]
  await operations.persist(next)
  try {
    operations.mirror?.(next)
  } catch (error) {
    operations.onMirrorError?.(error)
  }
  operations.publish(next)
}

export async function publishUpsertDramaRecord(
  records: AutoRecord[],
  record: AutoRecord,
  operations: {
    persist: (records: AutoRecord[]) => Promise<unknown>
    mirror?: (records: AutoRecord[]) => void
    onMirrorError?: (error: unknown) => void
    publish: (records: AutoRecord[]) => void
  },
) {
  const idx = records.findIndex((item) => item.path === record.path)
  const next =
    idx === -1
      ? [record, ...records]
      : records.map((item, i) => (i === idx ? record : item))
  await operations.persist(next)
  try {
    operations.mirror?.(next)
  } catch (error) {
    operations.onMirrorError?.(error)
  }
  operations.publish(next)
}
