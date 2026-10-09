import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
// $lib/search pulls in convertFileSrc alongside invoke; both must be provided
// or the module fails to resolve.
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

import TagPickerFixture from './fixtures/TagPickerFixture.svelte'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

const EMPTY_OPTIONS = {
  formats: [],
  languages: [],
  tags: [],
  genres: [],
  subjects: [],
  publishers: [],
  authors: [],
}

let tags: Array<{ id: string; label: string }>

beforeEach(() => {
  tags = [
    { id: 'tag-1', label: 'to-read' },
    { id: 'tag-2', label: 'reference' },
  ]
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'filter_options') return { ...EMPTY_OPTIONS, tags }
    throw new Error(`unexpected command: ${command}`)
  })
})

function input(container: HTMLElement): WaInput {
  return container.querySelector('wa-input') as WaInput
}

function addButton(container: HTMLElement): WaButton {
  return container.querySelector('.create wa-button') as WaButton
}

function tagLabels(container: HTMLElement): string[] {
  return [...container.querySelectorAll('ul li span')].map((n) => n.textContent?.trim() ?? '')
}

async function typeInto(container: HTMLElement, value: string) {
  const control = input(container)
  await control.updateComplete
  control.value = value
  control.dispatchEvent(new Event('input', { bubbles: true }))
  return control
}

/** wa-button swallows clicks while disabled, and `disabled` only clears once
 * Svelte has flushed the new draft, so wait for that before clicking. */
async function clickAdd(container: HTMLElement) {
  await expect.poll(() => addButton(container).disabled).toBe(false)
  addButton(container).click()
}

test('lists the tags the query returns', async () => {
  const { container } = await render(TagPickerFixture)

  await expect.poll(() => tagLabels(container)).toEqual(['to-read', 'reference'])
})

test('renders no tags when the library has none', async () => {
  tags = []

  const { container } = await render(TagPickerFixture)

  await expect.poll(() => invoke.mock.calls.length).toBeGreaterThan(0)
  expect(tagLabels(container)).toEqual([])
})

test('reports a new tag by name', async () => {
  const onadd = vi.fn()
  const { container } = await render(TagPickerFixture, { props: { onadd } })

  await typeInto(container, 'borrowed')
  await clickAdd(container)

  expect(onadd).toHaveBeenCalledWith('borrowed')
})

test('trims surrounding whitespace off a new tag', async () => {
  const onadd = vi.fn()
  const { container } = await render(TagPickerFixture, { props: { onadd } })

  await typeInto(container, '  borrowed  ')
  await clickAdd(container)

  expect(onadd).toHaveBeenCalledWith('borrowed')
})

// A blank draft must not create a tag. The button is disabled for it, but
// submit() guards independently because Enter bypasses the button.
test('ignores a draft that is only whitespace', async () => {
  const onadd = vi.fn()
  const { container } = await render(TagPickerFixture, { props: { onadd } })

  const control = await typeInto(container, '   ')
  control.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))

  expect(onadd).not.toHaveBeenCalled()
})

test('enter submits without using the button', async () => {
  const onadd = vi.fn()
  const { container } = await render(TagPickerFixture, { props: { onadd } })

  const control = await typeInto(container, 'borrowed')
  control.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))

  expect(onadd).toHaveBeenCalledWith('borrowed')
})

test('clears the draft after adding, so the next tag starts empty', async () => {
  const { container } = await render(TagPickerFixture, { props: { onadd: vi.fn() } })

  await typeInto(container, 'borrowed')
  await clickAdd(container)

  await expect.poll(() => input(container).value).toBe('')
})

test('removing a tag reports its id, not its label', async () => {
  const onremove = vi.fn()
  const { container } = await render(TagPickerFixture, { props: { onremove } })
  await expect.poll(() => tagLabels(container)).toEqual(['to-read', 'reference'])

  container.querySelectorAll<HTMLButtonElement>('button.remove')[1].click()

  expect(onremove).toHaveBeenCalledWith('tag-2')
})

test('the add button is disabled until the draft has content', async () => {
  const { container } = await render(TagPickerFixture)
  await addButton(container).updateComplete

  expect(addButton(container).disabled).toBe(true)

  await typeInto(container, 'borrowed')
  await expect.poll(() => addButton(container).disabled).toBe(false)

  await typeInto(container, '   ')
  await expect.poll(() => addButton(container).disabled).toBe(true)
})
