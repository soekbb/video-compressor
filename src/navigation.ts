import { ref } from 'vue'
import type { AppPage } from './types'

export const currentPage = ref<AppPage>('batch')

export function goToPage(page: AppPage) {
  currentPage.value = page
}
