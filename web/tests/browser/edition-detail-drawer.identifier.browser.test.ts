import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { EditionDetail } from '$lib/search'
import EditionDetailDrawerFixture from './fixtures/EditionDetailDrawerFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  // Typed so `mock.calls` keeps its `[command, args]` tuple shape.
  invoke: vi.fn(
    async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => null,
  ),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

const EDITION_ID = '01hzzzzzzzzzzzzzzzzzzzzzzz'
const ISBN = 'urn:isbn:9780802124675'
const OTHER = 'urn:other:abc'

function detail(overrides: Partial<EditionDetail> = {}): EditionDetail {
  return {
    id: EDITION_ID,
    work_id: '01hwwwwwwwwwwwwwwwwwwwwwww',
    title: 'Positive Obsession',
    published_date: '2016-01-01',
    format: 'EPUB',
    language_code: 'en',
    language_name: 'English',
    notes: null,
    description: null,
    authors: [],
    publishers: [],
    identifiers: [
      { kind: 'isbn', value: ISBN },
      { kind: 'other', value: OTHER },
    ],
    file: null,
    created_at: '2026-01-01',
    updated_at: null,
    ...overrides,
  }
}

beforeEach(() => {
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'update_edition_identifier' || command === 'remove_edition_identifier') {
      return null
    }
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => {
  // wa-drawer and wa-dialog escape the render container.
  for (const portal of document.querySelectorAll('wa-drawer, wa-dialog')) portal.remove()
})

function identifierRows(container: HTMLElement) {
  return [...container.querySelectorAll('.identifier-table tbody tr')] as HTMLElement[]
}

/** Open the drawer's edit dialog on row `index` by clicking its value cell. */
async function openIdentifierDialog(index = 0) {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect.poll(() => identifierRows(container).length).toBe(2)

  const value = identifierRows(container)[index].querySelector('.identifier-value')
  ;(value as HTMLElement).click()

  // The drawer also holds the review dialog; this one is labelled.
  const dialog = await vi.waitFor(() => {
    const found = document.querySelector('wa-dialog[label="Edit identifier"]')
    if (!found) throw new Error('identifier dialog did not open')
    return found
  })
  return { container, dialog: dialog as HTMLElement }
}

/**
 * Read a Web Awesome form control's value.
 *
 * The components are declared in `app.wa.js`, which the browser project does
 * not load, so Svelte writes `value` as a plain attribute here.
 */
function controlValue(dialog: HTMLElement, label: string) {
  const input = [...dialog.querySelectorAll('wa-input')].find(
    (candidate) => candidate.getAttribute('label') === label,
  )
  if (!input) throw new Error(`missing ${label} input`)
  return input.getAttribute('value') ?? ''
}

/** Fill a control and fire the event its listener reads. */
function setControl(dialog: HTMLElement, label: string, value: string) {
  const input = [...dialog.querySelectorAll('wa-input')].find(
    (candidate) => candidate.getAttribute('label') === label,
  ) as (HTMLElement & { value: string }) | null
  if (!input) throw new Error(`missing ${label} input`)
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

function submit(dialog: HTMLElement) {
  dialog
    .querySelector('form')
    ?.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }))
}

test('identifiers render as a kind / value table with per-row actions', async () => {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect.poll(() => identifierRows(container).length).toBe(2)

  const headers = [...container.querySelectorAll('.identifier-table th')].map((th) =>
    th.textContent?.trim(),
  )
  expect(headers).toEqual(['Kind', 'Value', 'Actions'])

  const [first, second] = identifierRows(container)
  expect(first.querySelector('.identifier-kind')?.textContent).toBe('isbn')
  expect(first.querySelector('.identifier-value')?.textContent).toBe('9780802124675')
  expect(second.querySelector('.identifier-kind')?.textContent).toBe('other')
  expect(first.querySelectorAll('.identifier-action').length).toBe(2)
})

test('clicking a value opens it prefilled with kind and value', async () => {
  const { dialog } = await openIdentifierDialog()

  expect(controlValue(dialog, 'Kind')).toBe('isbn')
  expect(controlValue(dialog, 'Value')).toBe('9780802124675')
})

test('saving an edited value sends the urn rebuilt from the fields', async () => {
  const { dialog } = await openIdentifierDialog()

  setControl(dialog, 'Value', '9780063211842')
  submit(dialog)

  await vi.waitFor(() => {
    const update = invoke.mock.calls.find(([command]) => command === 'update_edition_identifier')
    expect(update?.[1]).toEqual({
      editionId: EDITION_ID,
      kind: 'isbn',
      value: ISBN,
      newKind: 'isbn',
      newValue: 'urn:isbn:9780063211842',
    })
  })

  await expect
    .poll(() => document.querySelector('wa-dialog[label="Edit identifier"]')?.getAttribute('open'))
    .not.toBe('true')
})

test('an edited kind rewrites the urn scheme it is stored under', async () => {
  const { dialog } = await openIdentifierDialog()

  setControl(dialog, 'Kind', 'oclc')
  submit(dialog)

  await vi.waitFor(() => {
    const update = invoke.mock.calls.find(([command]) => command === 'update_edition_identifier')
    expect(update?.[1]).toMatchObject({ newKind: 'oclc', newValue: 'urn:oclc:9780802124675' })
  })
})

test('the row trash button asks before removing', async () => {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect.poll(() => identifierRows(container).length).toBe(2)

  const remove = identifierRows(container)[1].querySelector(
    '.identifier-action.is-remove',
  ) as HTMLElement
  remove.click()

  const confirm = await vi.waitFor(() => {
    const found = document.querySelector('wa-dialog[label="Remove identifier"]')
    if (!found) throw new Error('confirm dialog did not open')
    return found as HTMLElement
  })
  expect(confirm.textContent).toContain(OTHER)
  expect(invoke.mock.calls.map(([command]) => command).includes('remove_edition_identifier')).toBe(
    false,
  )

  ;(confirm.querySelector('.danger') as HTMLElement).click()

  await vi.waitFor(() => {
    const call = invoke.mock.calls.find(([command]) => command === 'remove_edition_identifier')
    expect(call?.[1]).toEqual({ editionId: EDITION_ID, kind: 'other', value: OTHER })
  })
})

test('a rejected edit keeps the dialog open with the typed value', async () => {
  const { dialog } = await openIdentifierDialog()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'update_edition_identifier') {
      return Promise.reject({
        IdentifierConflict: {
          value: 'urn:oclc:12345',
          existing_kind: 'oclc',
          requested_kind: 'lccn',
        },
      })
    }
    throw new Error(`unexpected command: ${command}`)
  })

  setControl(dialog, 'Value', '12345')
  submit(dialog)

  await vi.waitFor(() => {
    expect(invoke.mock.calls.map(([command]) => command)).toContain('update_edition_identifier')
  })
  expect(dialog.getAttribute('open')).toBe('true')
  expect(controlValue(dialog, 'Value')).toBe('12345')
})
