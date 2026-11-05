import '@awesome.me/webawesome/dist/components/badge/badge.js'
import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/callout/callout.js'
import '@awesome.me/webawesome/dist/components/card/card.js'
import '@awesome.me/webawesome/dist/components/dialog/dialog.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/spinner/spinner.js'
import '@awesome.me/webawesome/dist/components/textarea/textarea.js'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import ReadingListDetailFixture from './fixtures/ReadingListDetailFixture.svelte'
import ReadingListsPageFixture from './fixtures/ReadingListsPageFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))
const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

const LIST_ID = '01hzzzzzzzzzzzzzzzzzzzzzzz'
const EDITION_ID = '01heeeeeeeeeeeeeeeeeeeeeeeee'

function list(overrides: Record<string, unknown> = {}) {
  return {
    id: LIST_ID,
    name: 'Autumn',
    description: null,
    edition_ids: [] as string[],
    created_at: '2026-11-04T00:00:00Z',
    updated_at: null,
    ...overrides,
  }
}

function mockInvoke(handlers: Record<string, (args?: unknown) => unknown>) {
  invoke.mockImplementation(async (command: string, args?: unknown) => {
    if (command === 'atproto_status') return null
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler(args)
  })
}

type WaButton = HTMLElement & { updateComplete?: Promise<unknown>; textContent: string | null }

function buttonByText(container: HTMLElement, text: string): WaButton {
  const found = [...container.querySelectorAll<WaButton>('wa-button')].find((button) =>
    button.textContent?.includes(text),
  )
  expect(found, `a "${text}" button`).not.toBeUndefined()
  return found as WaButton
}

async function click(button: WaButton) {
  await button.updateComplete
  button.click()
}

async function fill(container: HTMLElement, selector: string, value: string) {
  const control = container.querySelector(selector) as HTMLElement & {
    value: string
    updateComplete?: Promise<unknown>
  }
  await control.updateComplete
  control.value = value
  control.dispatchEvent(new Event('input', { bubbles: true }))
  await control.updateComplete
}

test('creates a list from the index dialog and refreshes the grid', async () => {
  let created = false
  mockInvoke({
    list_reading_lists: () => (created ? [list({ name: 'Winter' })] : []),
    create_reading_list: (args) => {
      expect(args).toEqual({ name: 'Winter', description: null })
      created = true
      return list({ name: 'Winter' })
    },
  })

  const { container } = await render(ReadingListsPageFixture)

  await expect.poll(() => container.textContent).toContain('No reading lists yet.')
  await click(buttonByText(container, 'New list'))
  await expect.poll(() => container.querySelector('.create-form wa-input')).not.toBeNull()

  await fill(container, '.create-form wa-input', 'Winter')
  await expect.poll(() => buttonByText(container, 'Create').disabled).toBe(false)
  await click(buttonByText(container, 'Create'))

  await expect.poll(() => toast.success).toHaveBeenCalled()
  await expect.poll(() => container.textContent).toContain('Winter')
})

test('adds a book through the picker', async () => {
  mockInvoke({
    get_reading_list: () => list(),
    search_typeahead: () => [
      {
        work_id: '01hwwwwwwwwwwwwwwwwwwwwwww',
        edition_id: EDITION_ID,
        title: 'Dune',
        authors: ['Frank Herbert'],
        pub_date: null,
        snippet_text: null,
        kind: 'edition',
        has_file: false,
      },
    ],
    get_edition_detail: () => ({
      id: EDITION_ID,
      title: 'Dune',
      authors: [{ name: 'Frank Herbert', role: 'author' }],
    }),
    get_edition_covers: () => [{ edition_id: EDITION_ID, cover_path: '/covers/dune.jpg' }],
    add_reading_list_book: (args) => {
      expect(args).toEqual({ listId: LIST_ID, editionId: EDITION_ID })
      return list({ edition_ids: [EDITION_ID] })
    },
  })

  const { container } = await render(ReadingListDetailFixture)

  await expect.poll(() => buttonByText(container, 'Add book').disabled).toBe(false)
  await click(buttonByText(container, 'Add book'))
  await expect.poll(() => container.querySelector('.add-book wa-input')).not.toBeNull()

  await fill(container, '.add-book wa-input', 'Dune')
  await expect.poll(() => container.querySelector('.candidate')).not.toBeNull()
  await expect.poll(() => container.querySelector('.candidate-cover img')).not.toBeNull()
  await click(container.querySelector('.candidate') as WaButton)

  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('add_reading_list_book', {
      listId: LIST_ID,
      editionId: EDITION_ID,
    })
  await expect.poll(() => container.textContent).toContain('Dune')
})

test('removes a book from the list', async () => {
  mockInvoke({
    get_reading_list: () => list({ edition_ids: [EDITION_ID] }),
    get_edition_detail: () => ({
      id: EDITION_ID,
      title: 'Dune',
      authors: [{ name: 'Frank Herbert', role: 'author' }],
    }),
    remove_reading_list_book: (args) => {
      expect(args).toEqual({ listId: LIST_ID, editionId: EDITION_ID })
      return list()
    },
  })

  const { container } = await render(ReadingListDetailFixture)

  await expect.poll(() => container.textContent).toContain('Dune')
  await click(buttonByText(container, 'Remove'))

  await expect.poll(() => container.textContent).toContain('This list has no books yet.')
})

test('deletes the list after confirmation', async () => {
  mockInvoke({
    get_reading_list: () => list(),
    delete_reading_list: (args) => {
      expect(args).toEqual({ listId: LIST_ID })
      return true
    },
  })

  const { container } = await render(ReadingListDetailFixture)

  await expect.poll(() => container.querySelector('wa-dialog')).toBeNull()
  await click(buttonByText(container, 'Delete'))
  await expect.poll(() => container.querySelector('.danger')).not.toBeNull()
  await click(container.querySelector('.danger') as WaButton)

  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('delete_reading_list', {
      listId: LIST_ID,
    })
})
