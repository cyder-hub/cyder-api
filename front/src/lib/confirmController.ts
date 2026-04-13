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
  pendingAction: 'confirm' | 'cancel' | null
}

const state = reactive<ConfirmState>({
  isOpen: false,
  options: {
    title: '',
    description: '',
  },
  resolve: null,
  pendingAction: null,
})

export function confirm(options: ConfirmOptions | string): Promise<boolean> {
  const confirmOptions: ConfirmOptions =
    typeof options === 'string' ? { title: options } : options

  state.options = {
    ...confirmOptions,
  }
  state.pendingAction = null
  state.isOpen = true

  return new Promise<boolean>((res) => {
    state.resolve = res
  })
}

export function markConfirmIntent() {
  state.pendingAction = 'confirm'
}

export function markCancelIntent() {
  state.pendingAction = 'cancel'
}

function settle(value: boolean) {
  state.isOpen = false
  if (state.resolve) {
    state.resolve(value)
    state.resolve = null
  }
  state.pendingAction = null
}

export function handleConfirm() {
  settle(true)
}

export function handleCancel() {
  settle(false)
}

export function handleOpenChange(open: boolean) {
  if (open) {
    state.isOpen = true
    return
  }

  if (state.pendingAction === 'confirm') {
    handleConfirm()
    return
  }

  handleCancel()
}

export function useConfirmState() {
  return state
}
