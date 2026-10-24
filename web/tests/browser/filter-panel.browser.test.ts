import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { EditionFilters } from '$lib/search'
import FilterPanelFixture from './fixtures/FilterPanelFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

const OPTIONS = {
  formats: [{ id: 'epub', label: 'EPUB' }],
  languages: [{ id: 'en', label: 'English' }],
  tags: [],
  genres: [],
  subjects: [],
  publishers: [],
  authors: [],
}

let seen: EditionFilters[]

beforeEach(() => {
  seen = []
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'filter_options') return OPTIONS
    throw new Error(`unexpected command: ${command}`)
  })
})

function selectByLabel(container: HTMLElement, label: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>('wa-select')].find(
    (node) => node.getAttribute('aria-label') === label,
  )
  expect(found, `a "${label}" select`).not.toBeUndefined()
  return found as HTMLElement
}

function choose(node: HTMLElement, value: string) {
  ;(node as HTMLElement & { value: string }).value = value
  node.dispatchEvent(new Event('change', { bubbles: true }))
}

function buttonByLabel(container: HTMLElement, label: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>('wa-button')].find(
    (node) => node.textContent?.trim() === label,
  )
  expect(found, `a "${label}" button`).not.toBeUndefined()
  return found as HTMLElement
}

test('renders availability, axis, and sort selects once options arrive', async () => {
  const { container } = render(FilterPanelFixture)

  await expect.poll(() => selectByLabel(container, 'Availability')).not.toBeUndefined()
  expect(container.textContent).toContain('Format')
  expect(selectByLabel(container, 'Sort field')).not.toBeUndefined()
  expect(selectByLabel(container, 'Sort direction')).not.toBeUndefined()
})

test('choosing a sort field reports it through onchange', async () => {
  const { container } = render(FilterPanelFixture, {
    props: { onchange: (filters: EditionFilters) => seen.push(filters) },
  })
  await expect.poll(() => selectByLabel(container, 'Sort field')).not.toBeUndefined()

  choose(selectByLabel(container, 'Sort field'), 'title')

  await expect.poll(() => seen.length).toBe(1)
  expect(seen[0]).toMatchObject({ sort_by: 'title' })
})

test('choosing availability reports has_file through onchange', async () => {
  const { container } = render(FilterPanelFixture, {
    props: { onchange: (filters: EditionFilters) => seen.push(filters) },
  })
  await expect.poll(() => selectByLabel(container, 'Availability')).not.toBeUndefined()

  choose(selectByLabel(container, 'Availability'), 'true')

  await expect.poll(() => seen.length).toBe(1)
  expect(seen[0]).toMatchObject({ has_file: true })
})

test('choosing availability false reports has_file false', async () => {
  const { container } = render(FilterPanelFixture, {
    props: { onchange: (filters: EditionFilters) => seen.push(filters) },
  })
  await expect.poll(() => selectByLabel(container, 'Availability')).not.toBeUndefined()

  choose(selectByLabel(container, 'Availability'), 'false')

  await expect.poll(() => seen.length).toBe(1)
  expect(seen[0]).toMatchObject({ has_file: false })
})
