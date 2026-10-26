import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import AnnotationComposerFixture from './fixtures/AnnotationComposerFixture.svelte'

type WaInput = HTMLElement & { value: string }
type WaButton = HTMLElement & { disabled: boolean }

function input(container: HTMLElement): WaInput {
  const element = container.querySelector('wa-input')
  if (!element) throw new Error('composer has no input')
  return element as WaInput
}

function saveButton(container: HTMLElement): WaButton {
  const element = container.querySelector('wa-button')
  if (!element) throw new Error('composer has no save button')
  return element as WaButton
}

test('saving is refused until the note has text', async () => {
  const saved: string[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  expect(saveButton(container).disabled).toBe(true)

  input(container).value = 'Key point'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))

  await expect.poll(() => saveButton(container).disabled).toBe(false)
})

test('saves the trimmed note text', async () => {
  const saved: string[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  input(container).value = '  Key point  '
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  await expect.poll(() => saveButton(container).disabled).toBe(false)

  saveButton(container).click()

  await expect.poll(() => saved).toEqual(['Key point'])
})

test('shows the selected text as a quote', async () => {
  const saved: string[] = []
  const { container } = await render(AnnotationComposerFixture, {
    saved,
    quote: 'The selected sentence',
  })

  expect(container.querySelector('blockquote')?.textContent).toContain('The selected sentence')
})

test('enter saves and escape cancels', async () => {
  const saved: string[] = []
  const { container } = await render(AnnotationComposerFixture, { saved })

  input(container).value = 'From the keyboard'
  input(container).dispatchEvent(new Event('input', { bubbles: true }))
  input(container).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  await expect.poll(() => saved).toEqual(['From the keyboard'])

  input(container).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
  await expect.poll(() => saved).toEqual(['From the keyboard', '<cancel>'])
})
