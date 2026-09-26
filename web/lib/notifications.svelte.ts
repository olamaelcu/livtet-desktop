import type { ExternalToast } from 'svelte-sonner'
import { toast } from 'svelte-sonner'

export type NotifyKind = 'success' | 'info' | 'warning' | 'error'
export type ToastId = number | string

export interface HistoryEntry {
  id: ToastId
  kind: NotifyKind
  title: string
  description?: string
  at: number
}

export const MAX_HISTORY = 50

export const history = $state<HistoryEntry[]>([])
export const ui = $state({ historyOpen: false })

export function openHistory() {
  ui.historyOpen = true
}

function record(kind: NotifyKind, title: string, description: unknown, id: ToastId) {
  history.unshift({
    id,
    kind,
    title,
    description: typeof description === 'string' ? description : undefined,
    at: Date.now(),
  })
  if (history.length > MAX_HISTORY) history.length = MAX_HISTORY
}

function show(kind: NotifyKind, title: string, opts?: ExternalToast): ToastId | undefined {
  try {
    const options: ExternalToast = { ...opts }
    if (!options.action) {
      options.action = { label: 'Show all', onClick: () => openHistory() }
    }
    const id = toast[kind](title, options) as ToastId
    record(kind, title, opts?.description, id)
    return id
  } catch (error) {
    console.error('notify failed', error)
    return undefined
  }
}

export const notify = {
  success: (title: string, opts?: ExternalToast) => show('success', title, opts),
  info: (title: string, opts?: ExternalToast) => show('info', title, opts),
  warning: (title: string, opts?: ExternalToast) => show('warning', title, opts),
  error: (title: string, opts?: ExternalToast) => show('error', title, opts),
}

export function dismissEntry(id: ToastId) {
  toast.dismiss(id)
  const index = history.findIndex((entry) => entry.id === id)
  if (index >= 0) history.splice(index, 1)
}

export function dismissAll() {
  toast.dismiss()
  history.length = 0
}
