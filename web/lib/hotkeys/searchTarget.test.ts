import { beforeEach, expect, test, vi } from 'vitest'

const { goto } = vi.hoisted(() => ({ goto: vi.fn(async () => {}) }))
vi.mock('$app/navigation', () => ({ goto }))
vi.mock('$app/paths', () => ({ resolve: (path: string) => path }))

import { focusSearchTarget, registerSearchTarget } from './searchTarget.svelte'

beforeEach(() => {
  goto.mockClear()
})

test('without a target it navigates to the library', async () => {
  await focusSearchTarget()

  expect(goto).toHaveBeenCalledWith('/library')
})

test('a registered target is focused instead of navigating', async () => {
  const focus = vi.fn()
  const dispose = registerSearchTarget(focus)

  await focusSearchTarget()

  expect(focus).toHaveBeenCalled()
  expect(goto).not.toHaveBeenCalled()
  dispose()
})

test('disposing unregisters so focus navigates again', async () => {
  const dispose = registerSearchTarget(vi.fn())
  dispose()

  await focusSearchTarget()

  expect(goto).toHaveBeenCalledWith('/library')
})

test('disposing a stale registration leaves the current target alone', async () => {
  const first = registerSearchTarget(vi.fn())
  const focus = vi.fn()
  registerSearchTarget(focus)
  first()

  await focusSearchTarget()

  expect(focus).toHaveBeenCalled()
  expect(goto).not.toHaveBeenCalled()
})
