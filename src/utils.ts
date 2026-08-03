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

export function makeId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`
}

export function isVideoFileName(name: string) {
  return /\.(mp4|mov|mkv|avi|webm|m4v|wmv|flv)$/i.test(name)
}
