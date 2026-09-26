import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createClickDisambiguation } from './clickDisambiguation'

describe('createClickDisambiguation', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('fires a single click after the timeout', () => {
    const onSingle = vi.fn()
    const press = createClickDisambiguation({ onSingle, onDouble: vi.fn() })
    press.handleClick({ detail: 1 })
    expect(onSingle).not.toHaveBeenCalled()
    vi.advanceTimersByTime(250)
    expect(onSingle).toHaveBeenCalledTimes(1)
  })

  it('cancels the pending single click and fires only the double-click callback', () => {
    const onSingle = vi.fn()
    const onDouble = vi.fn()
    const press = createClickDisambiguation({ onSingle, onDouble })
    press.handleClick({ detail: 1 })
    press.handleClick({ detail: 1 })
    press.handleDoubleClick()
    vi.advanceTimersByTime(1000)
    expect(onSingle).not.toHaveBeenCalled()
    expect(onDouble).toHaveBeenCalledTimes(1)
  })

  it('invokes single click immediately when no double-click handler exists', () => {
    const onSingle = vi.fn()
    const press = createClickDisambiguation({ onSingle })
    press.handleClick({ detail: 1 })
    expect(onSingle).toHaveBeenCalledTimes(1)
    press.handleDoubleClick()
    vi.advanceTimersByTime(1000)
    expect(onSingle).toHaveBeenCalledTimes(1)
  })

  it('invokes keyboard activation immediately without waiting', () => {
    const onSingle = vi.fn()
    const press = createClickDisambiguation({ onSingle, onDouble: vi.fn() })
    press.handleClick({ detail: 0 })
    expect(onSingle).toHaveBeenCalledTimes(1)
  })

  it('drops a pending single click on dispose', () => {
    const onSingle = vi.fn()
    const press = createClickDisambiguation({ onSingle, onDouble: vi.fn() })
    press.handleClick({ detail: 1 })
    press.dispose()
    vi.advanceTimersByTime(1000)
    expect(onSingle).not.toHaveBeenCalled()
  })
})
