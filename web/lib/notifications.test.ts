import { beforeEach, describe, expect, it, vi } from 'vitest'

const { success, error, warning, info, dismiss } = vi.hoisted(() => ({
  success: vi.fn((_title?: string, _opts?: unknown) => 7),
  error: vi.fn((_title?: string, _opts?: unknown) => 8),
  warning: vi.fn((_title?: string, _opts?: unknown) => 9),
  info: vi.fn((_title?: string, _opts?: unknown) => 10),
  dismiss: vi.fn(),
}))
vi.mock('svelte-sonner', () => ({
  toast: { success, error, warning, info, dismiss },
}))

import { dismissAll, history, notify, ui } from './notifications.svelte'

beforeEach(() => {
  vi.clearAllMocks()
  dismissAll()
  ui.historyOpen = false
})

describe('notify', () => {
  it('records kind, title, and string description, newest first', () => {
    notify.error('Nope', { description: 'gone' })
    expect(history.length).toBe(1)
    expect(history[0]).toMatchObject({ kind: 'error', title: 'Nope', description: 'gone', id: 8 })
    expect(typeof history[0].at).toBe('number')
  })

  it('appends a Show-all action only when the caller passed none', () => {
    notify.success('Hi')
    expect(success).toHaveBeenCalledOnce()
    const opts = success.mock.calls[0][1] as {
      action: { label: string; onClick: (e: MouseEvent) => void }
    }
    expect(opts.action.label).toBe('Show all')
    opts.action.onClick({} as MouseEvent)
    expect(ui.historyOpen).toBe(true)
  })

  it('never clobbers a caller action', () => {
    const mine = { label: 'Undo', onClick: () => {} }
    notify.success('Hi', { action: mine })
    expect(success.mock.calls[0][1]).toMatchObject({ action: mine })
  })

  it('caps the log at 50, evicting oldest', () => {
    for (let i = 0; i < 55; i++) notify.info(`t${i}`)
    expect(history.length).toBe(50)
    expect(history[0].title).toBe('t54')
    expect(history[49].title).toBe('t5')
  })

  it('never throws when sonner fails', () => {
    success.mockImplementationOnce(() => {
      throw new Error('boom')
    })
    let result: unknown
    expect(() => {
      result = notify.success('Hi')
    }).not.toThrow()
    expect(result).toBeUndefined()
    expect(history.length).toBe(0)
  })
})

describe('dismissAll', () => {
  it('clears sonner and the log', () => {
    notify.warning('W')
    dismissAll()
    expect(dismiss).toHaveBeenCalledWith()
    expect(history.length).toBe(0)
  })
})
