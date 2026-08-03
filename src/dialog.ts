import { readonly, ref } from 'vue'

export type DialogTone = 'info' | 'ok' | 'warn' | 'danger'

export type DialogItem = {
  title: string
  meta?: string
}

export type DialogOptions = {
  title?: string
  message: string
  confirmText?: string
  cancelText?: string
  tone?: DialogTone
  /** 结构化列表，如扫描到的剧目 */
  items?: DialogItem[]
  itemsMore?: number
}

type DialogState = {
  open: boolean
  mode: 'alert' | 'confirm'
  title: string
  message: string
  confirmText: string
  cancelText: string
  tone: DialogTone
  items: DialogItem[]
  itemsMore: number
}

const defaults = {
  title: '提示',
  confirmText: '知道了',
  cancelText: '取消',
  tone: 'info' as DialogTone,
}

const state = ref<DialogState>({
  open: false,
  mode: 'alert',
  message: '',
  items: [],
  itemsMore: 0,
  ...defaults,
})

let resolver: ((value: boolean) => void) | null = null

export const dialogState = readonly(state)

function openDialog(
  options: DialogOptions,
  mode: 'alert' | 'confirm',
): Promise<boolean> {
  if (resolver) {
    resolver(false)
    resolver = null
  }
  state.value = {
    open: true,
    mode,
    title: options.title ?? defaults.title,
    message: options.message,
    confirmText: options.confirmText ?? (mode === 'confirm' ? '确定' : defaults.confirmText),
    cancelText: options.cancelText ?? defaults.cancelText,
    tone: options.tone ?? defaults.tone,
    items: options.items ?? [],
    itemsMore: options.itemsMore ?? 0,
  }
  return new Promise((resolve) => {
    resolver = resolve
  })
}

export function showDialog(options: DialogOptions): Promise<void> {
  return openDialog(options, 'alert').then(() => undefined)
}

export function showConfirm(options: DialogOptions): Promise<boolean> {
  return openDialog(options, 'confirm')
}

export function closeDialog(confirmed = true) {
  if (!state.value.open) return
  state.value = { ...state.value, open: false }
  const done = resolver
  resolver = null
  done?.(confirmed)
}
