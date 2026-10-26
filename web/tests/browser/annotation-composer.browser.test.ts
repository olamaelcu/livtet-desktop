import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { HighlightColor } from '$lib/reader/highlights'
import AnnotationComposerFixture from './fixtures/AnnotationComposerFixture.svelte'

type SavedNote = { text: string; color: HighlightColor | undefined }

type WaInput = HTMLElement & { value: string }
type WaButton = HTMLElement & { disabled: boolean }

function input(container: HTMLElement): WaInput {
  const element = container.querySelector('wa-input')
  if (!element) throw new Error('composer has no input')
  return element as WaInput
}

/** Cancel is first in the action row; the save action is last, whatever its label. */
function saveButton(container: HTMLElement): WaButton {
  const actions = container.querySelectorAll('.actions wa-button')
  const element = actions[actions.length - 1]
  if (!element) throw new Error('composer has no save button')
  return element as WaButton
}

test('saving is refused until the note has text', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  expect(saveButton(container).disabled).toBe(true)

  input(container).value = 'Key point'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))

  await expect.poll(() => saveButton(container).disabled).toBe(false)
})

test('saves the trimmed note text', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  input(container).value = '  Key point  '
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  await expect.poll(() => saveButton(container).disabled).toBe(false)

  saveButton(container).click()

  await expect.poll(() => saved).toEqual([{ text: 'Key point', color: undefined }])
})

test('shows the selected text as a quote', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, {
    saved,
    quote: 'The selected sentence',
  })

  expect(container.querySelector('blockquote')?.textContent).toContain('The selected sentence')
})

test('enter saves and escape cancels', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  input(container).value = 'From the keyboard'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  input(container).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  await expect.poll(() => saved).toEqual([{ text: 'From the keyboard', color: undefined }])

  input(container).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
  await expect
    .poll(() => saved)
    .toEqual([
      { text: 'From the keyboard', color: undefined },
      { text: '<cancel>', color: undefined },
    ])
})

test('saves the chosen highlight colour', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, { saved, showColors: true })

  const swatches = [...container.querySelectorAll('.swatch')] as HTMLElement[]
  expect(swatches.length).toBeGreaterThan(1)

  // Default is preselected; pick a different one.
  const chosen = swatches[2]
  chosen.click()
  await expect.poll(() => chosen.getAttribute('aria-pressed')).toBe('true')

  input(container).value = 'Green note'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  await expect.poll(() => saveButton(container).disabled).toBe(false)
  saveButton(container).click()

  await expect.poll(() => saved).toEqual([{ text: 'Green note', color: 'blue' }])
})

test('offers no colour palette when the reader cannot paint one', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  expect(container.querySelectorAll('.swatch').length).toBe(0)
})

test('prefills an existing note and saves the edit', async () => {
  const saved: SavedNote[] = []
  const { container } = await render(AnnotationComposerFixture, {
    saved,
    showColors: true,
    initialText: 'Original text',
    initialColor: 'pink',
  })

  expect(input(container).value).toBe('Original text')
  expect(container.querySelector('wa-dialog')?.getAttribute('label')).toBe('Edit note')

  const pink = [...container.querySelectorAll('.swatch')][3]
  expect(pink.getAttribute('aria-pressed')).toBe('true')

  input(container).value = 'Revised text'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  await expect.poll(() => saveButton(container).disabled).toBe(false)
  saveButton(container).click()

  await expect.poll(() => saved).toEqual([{ text: 'Revised text', color: 'pink' }])
})
