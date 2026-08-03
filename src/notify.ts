import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from '@tauri-apps/plugin-notification'
import { isTauri } from './desktop'

let permissionPromise: Promise<boolean> | null = null

async function ensurePermission(): Promise<boolean> {
  if (!isTauri()) return false
  if (!permissionPromise) {
    permissionPromise = (async () => {
      try {
        let granted = await isPermissionGranted()
        if (!granted) {
          granted = (await requestPermission()) === 'granted'
        }
        return granted
      } catch (e) {
        console.error('通知权限检查失败', e)
        return false
      }
    })()
  }
  return permissionPromise
}

/** 请求通知权限（启动时预热，避免首次完成时才弹系统对话框） */
export async function initNotifications() {
  await ensurePermission()
}

export async function notifyDesktop(title: string, body: string) {
  if (!(await ensurePermission())) return
  try {
    sendNotification({ title, body })
  } catch (e) {
    console.error('发送系统通知失败', e)
  }
}
