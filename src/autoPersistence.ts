import type { AutoRecord } from './types.ts'

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
