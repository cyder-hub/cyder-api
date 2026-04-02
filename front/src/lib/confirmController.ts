import { reactive } from 'vue'

interface ConfirmOptions {
  title: string
  description?: string
  confirmText?: string
  cancelText?: string
}

interface ConfirmState {
  isOpen: boolean
  options: ConfirmOptions
  resolve: ((value: boolean) => void) | null
}

const state = reactive<ConfirmState>({
  isOpen: false,
  options: {
    title: '',
    description: '',
  },
  resolve: null,
})

export function confirm(options: ConfirmOptions | string): Promise<boolean> {
  const confirmOptions: ConfirmOptions =
    typeof options === 'string' ? { title: options } : options

  state.options = {
    ...confirmOptions,
  }
  state.isOpen = true

  return new Promise<boolean>((res) => {
    state.resolve = res
  })
}

export function handleConfirm() {
  state.isOpen = false
  if (state.resolve) {
    state.resolve(true)
    state.resolve = null
  }
}

export function handleCancel() {
  state.isOpen = false
  if (state.resolve) {
    state.resolve(false)
    state.resolve = null
  }
}

export function useConfirmState() {
  return state
}
