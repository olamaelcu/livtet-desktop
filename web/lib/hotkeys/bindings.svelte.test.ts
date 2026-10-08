import { afterEach, describe, expect, test } from 'vitest'
import {
  bindingFor,
  hasOverride,
  initBindings,
  resetBindings,
  setBinding,
} from '$lib/hotkeys/bindings.svelte'

// Overrides live in module state shared across tests, so each test clears it.
// `persist()` has its own catch for running outside Tauri, so no store mock is
// needed — the write simply goes nowhere.
afterEach(() => {
  resetBindings()
})

describe('bindingFor', () => {
  test('returns the command default when no override is set', () => {
    expect(bindingFor('palette.open')).toBe('Mod+K')
    expect(bindingFor('search.focus')).toBe('/')
  })

  test('returns the override once one is set', () => {
    setBinding('palette.open', 'Mod+P')

    expect(bindingFor('palette.open')).toBe('Mod+P')
  })

  test('leaves other commands on their defaults', () => {
    setBinding('palette.open', 'Mod+P')

    expect(bindingFor('nav.library')).toBe('Mod+1')
  })
})

describe('hasOverride', () => {
  test('is false for a command left on its default', () => {
    expect(hasOverride('app.refresh')).toBe(false)
  })

  test('is true once the command is rebound', () => {
    setBinding('app.refresh', 'F5')

    expect(hasOverride('app.refresh')).toBe(true)
  })
})

describe('resetBindings', () => {
  test('clears every override', () => {
    setBinding('palette.open', 'Mod+P')
    setBinding('nav.settings', 'Mod+,')

    resetBindings()

    expect(hasOverride('palette.open')).toBe(false)
    expect(hasOverride('nav.settings')).toBe(false)
    expect(bindingFor('palette.open')).toBe('Mod+K')
  })
})

describe('initBindings', () => {
  // There is no Tauri store outside the app, so loading falls into the catch
  // and the defaults stay in effect rather than the call throwing.
  test('leaves defaults in place when no store is reachable', async () => {
    await expect(initBindings()).resolves.toBeUndefined()

    expect(bindingFor('palette.open')).toBe('Mod+K')
    expect(hasOverride('palette.open')).toBe(false)
  })
})
