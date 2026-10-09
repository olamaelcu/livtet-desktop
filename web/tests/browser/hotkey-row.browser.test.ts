import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { afterEach, beforeEach, expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import HotkeyRow from '$lib/components/HotkeyRow.svelte'
import { resetBindings, setBinding } from '$lib/hotkeys/bindings.svelte'
import { COMMAND_BY_ID } from '$lib/hotkeys/commands'

// No store mock: setBinding persists through @tauri-apps/plugin-store, and
// bindings.svelte.ts catches that failure itself. Mocking it only gave Vite a
// new dependency to optimize mid-run, which reloaded the suite and failed
// every test once before passing on retry.
const COMMAND = COMMAND_BY_ID['palette.open']

beforeEach(() => {
  resetBindings()
})

afterEach(() => {
  resetBindings()
})

function label(container: HTMLElement): string {
  return container.querySelector('.hotkey-label')?.textContent?.trim() ?? ''
}

function binding(container: HTMLElement): string {
  return container.querySelector('.hotkey-binding')?.textContent?.trim() ?? ''
}

function actionLabels(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.hotkey-actions wa-button')].map(
    (node) => node.textContent?.replace(/\s+/g, ' ').trim() ?? '',
  )
}

test('renders the command label and its current binding', async () => {
  const { container } = await render(HotkeyRow, { props: { command: COMMAND } })

  expect(label(container)).toBe('Open command palette')
  // Mod+K is displayed per-platform, so assert it resolved to something
  // containing the key rather than a fixed modifier glyph.
  expect(binding(container)).toContain('K')
})

test('shows the override rather than the default once one is set', async () => {
  setBinding('palette.open', 'Mod+J')

  const { container } = await render(HotkeyRow, { props: { command: COMMAND } })

  expect(binding(container)).toContain('J')
  expect(binding(container)).not.toContain('K')
})

test('offers recording while idle', async () => {
  const { container } = await render(HotkeyRow, { props: { command: COMMAND } })

  expect(actionLabels(container)).toEqual(['Record'])
})

test('swaps to a prompt and a cancel action while recording', async () => {
  const { container } = await render(HotkeyRow, { props: { command: COMMAND } })
  const record = container.querySelector<HTMLElement & { updateComplete?: Promise<unknown> }>(
    '.hotkey-actions wa-button',
  )
  expect(record, 'the idle row offers a record button').not.toBeNull()
  await record?.updateComplete
  record?.click()

  await expect.poll(() => actionLabels(container)).toEqual(['Cancel'])
  expect(binding(container)).toBe('Press keys…')
})
