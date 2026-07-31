import { describe, expect, it, vi } from 'vitest'
import { attachActivate, attachAsButton } from './attachments'

function mockKeyboardEvent(key: string): KeyboardEvent {
  const event = new Event('keydown', { bubbles: true, cancelable: true }) as KeyboardEvent
  Object.defineProperty(event, 'key', { value: key })
  vi.spyOn(event, 'preventDefault')
  return event
}

function makeNode(): HTMLElement {
  const listeners: Record<string, EventListener[]> = {}
  const attributes: Record<string, string | null> = {}

  return {
    addEventListener: vi.fn((event: string, handler: EventListener) => {
      listeners[event] ??= []
      listeners[event].push(handler)
    }),
    removeEventListener: vi.fn((event: string, handler: EventListener) => {
      const list = listeners[event]
      if (list) {
        const idx = list.indexOf(handler)
        if (idx !== -1) list.splice(idx, 1)
      }
    }),
    click: vi.fn(),
    hasAttribute: vi.fn((name: string) => name in attributes),
    getAttribute: vi.fn((name: string) => attributes[name] ?? null),
    setAttribute: vi.fn((name: string, value: string) => {
      attributes[name] = value
    }),
    dispatchEvent: vi.fn(),
    _listeners: listeners,
    _attributes: attributes,
  } as unknown as HTMLElement
}

describe('attachActivate', () => {
  it('binds a keydown listener and returns a cleanup function', () => {
    const node = makeNode()
    const cleanup = attachActivate(node)
    expect(node.addEventListener).toHaveBeenCalledWith('keydown', expect.any(Function))

    cleanup()
    expect(node.removeEventListener).toHaveBeenCalledWith('keydown', expect.any(Function))
  })

  it('calls node.click() on Enter key', () => {
    const node = makeNode()
    attachActivate(node)

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    handler(mockKeyboardEvent('Enter'))

    expect(node.click).toHaveBeenCalled()
  })

  it('calls node.click() on Space key', () => {
    const node = makeNode()
    attachActivate(node)

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    handler(mockKeyboardEvent(' '))

    expect(node.click).toHaveBeenCalled()
  })

  it('calls onActivate callback when provided', () => {
    const node = makeNode()
    const onActivate = vi.fn()
    attachActivate(node, { onActivate })

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    handler(mockKeyboardEvent('Enter'))

    expect(onActivate).toHaveBeenCalled()
    expect(node.click).not.toHaveBeenCalled()
  })

  it('does nothing on other keys', () => {
    const node = makeNode()
    attachActivate(node)

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    handler(mockKeyboardEvent('A'))

    expect(node.click).not.toHaveBeenCalled()
  })

  it('calls preventDefault on Enter/Space', () => {
    const node = makeNode()
    attachActivate(node)

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    const event = mockKeyboardEvent('Enter')
    handler(event)

    expect(event.preventDefault).toHaveBeenCalled()
  })
})

describe('attachAsButton', () => {
  it('sets role to button if not set', () => {
    const node = makeNode()
    ;(node.hasAttribute as ReturnType<typeof vi.fn>).mockReturnValue(false)
    attachAsButton(node)

    expect(node.setAttribute).toHaveBeenCalledWith('role', 'button')
  })

  it('sets tabindex to 0 if not set', () => {
    const node = makeNode()
    ;(node.hasAttribute as ReturnType<typeof vi.fn>).mockReturnValue(false)
    attachAsButton(node)

    expect(node.setAttribute).toHaveBeenCalledWith('tabindex', '0')
  })

  it('does not override existing role', () => {
    const node = makeNode()
    ;(node.hasAttribute as ReturnType<typeof vi.fn>).mockImplementation(
      (name: string) => name === 'role',
    )
    attachAsButton(node)

    expect(node.setAttribute).not.toHaveBeenCalledWith('role', expect.any(String))
  })

  it('delegates keydown to attachActivate', () => {
    const node = makeNode()
    ;(node.hasAttribute as ReturnType<typeof vi.fn>).mockReturnValue(false)
    attachAsButton(node)

    const handler = (node.addEventListener as ReturnType<typeof vi.fn>).mock.calls.find(
      (c: [string]) => c[0] === 'keydown',
    )?.[1] as (e: KeyboardEvent) => void
    handler(mockKeyboardEvent('Enter'))

    expect(node.click).toHaveBeenCalled()
  })
})
