import { describe, expect, test } from 'vitest'
import { syncErrorMessage } from '$lib/sync'

describe('syncErrorMessage', () => {
  test('reports the message from an Unavailable error', () => {
    expect(syncErrorMessage({ Unavailable: { message: 'daemon not running' } })).toBe(
      'daemon not running',
    )
  })

  test('reports the message from an Rpc error', () => {
    expect(syncErrorMessage({ Rpc: { message: 'method not found' } })).toBe('method not found')
  })

  test('reports the message from a Protocol error', () => {
    expect(syncErrorMessage({ Protocol: { message: 'unexpected payload' } })).toBe(
      'unexpected payload',
    )
  })

  test('falls back to the message of a plain Error', () => {
    expect(syncErrorMessage(new Error('socket closed'))).toBe('socket closed')
  })

  test('stringifies anything else', () => {
    expect(syncErrorMessage('plain string')).toBe('plain string')
    expect(syncErrorMessage(42)).toBe('42')
    expect(syncErrorMessage(null)).toBe('null')
  })

  // The guard accepts any object carrying one of the three keys, but each
  // branch additionally requires the value to be truthy. A variant present but
  // empty therefore falls past all three and lands on String(error), which
  // renders as "[object Object]" — no use in a toast.
  test('renders an object shape with an empty variant as [object Object]', () => {
    expect(syncErrorMessage({ Rpc: null })).toBe('[object Object]')
  })
})
