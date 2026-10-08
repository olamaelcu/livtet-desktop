import { describe, expect, test } from 'vitest'
import { opdsErrorMessage } from '$lib/opds'

describe('opdsErrorMessage', () => {
  test('returns a string error unchanged', () => {
    expect(opdsErrorMessage('catalog unreachable')).toBe('catalog unreachable')
  })

  test('reads a string value out of an error object', () => {
    expect(opdsErrorMessage({ Network: 'connection refused' })).toBe('connection refused')
  })

  test('reads a nested message property', () => {
    expect(opdsErrorMessage({ Http: { message: '404 Not Found' } })).toBe('404 Not Found')
  })

  test('falls back to a generic message', () => {
    expect(opdsErrorMessage(null)).toBe('OPDS request failed')
    expect(opdsErrorMessage(undefined)).toBe('OPDS request failed')
    expect(opdsErrorMessage(7)).toBe('OPDS request failed')
    expect(opdsErrorMessage({})).toBe('OPDS request failed')
  })

  // It reads Object.values(error)[0], so the result depends on key insertion
  // order rather than on which key holds the message. An error whose first
  // value is a code returns the code, and a caller putting the message second
  // never sees it.
  test('returns the first value even when the message is not first', () => {
    expect(opdsErrorMessage({ code: 'E_NET', message: 'timed out' })).toBe('E_NET')
  })

  // A nested object whose first value lacks `message` also falls through,
  // because only the `message` key is unwrapped.
  test('falls back when a nested object carries no message key', () => {
    expect(opdsErrorMessage({ Http: { status: 404 } })).toBe('OPDS request failed')
  })
})
