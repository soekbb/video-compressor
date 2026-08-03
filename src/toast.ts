import { readonly, ref } from 'vue'
import { makeId } from './utils'

export type ToastTone = 'info' | 'ok' | 'warn' | 'danger'

export type ToastItem = {
  id: string
  message: string
  tone: ToastTone
  actionLabel?: string
  onAction?: () => void
}

const toasts = ref<ToastItem[]>([])
const timers = new Map<string, number>()

export const toastList = readonly(toasts)

export function showToast(options: {
  message: string
  tone?: ToastTone
  durationMs?: number
  actionLabel?: string
  onAction?: () => void
}) {
  const id = makeId()
  const item: ToastItem = {
    id,
    message: options.message,
    tone: options.tone ?? 'info',
    actionLabel: options.actionLabel,
    onAction: options.onAction,
  }
  toasts.value = [...toasts.value, item].slice(-3)

  const duration = options.durationMs ?? 4200
  if (duration > 0) {
    timers.set(
      id,
      window.setTimeout(() => dismissToast(id), duration),
    )
  }
  return id
}

export function dismissToast(id: string) {
  const timer = timers.get(id)
  if (timer) {
    window.clearTimeout(timer)
    timers.delete(id)
  }
  toasts.value = toasts.value.filter((t) => t.id !== id)
}

export function runToastAction(id: string) {
  const item = toasts.value.find((t) => t.id === id)
  item?.onAction?.()
  dismissToast(id)
}
