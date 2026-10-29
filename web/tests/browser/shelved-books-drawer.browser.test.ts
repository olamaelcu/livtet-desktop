import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { ShelvedBook } from '$lib/shelves'
import ShelvedBooksDrawerFixture from './fixtures/ShelvedBooksDrawerFixture.svelte'

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(async (_command: string): Promise<unknown> => []),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

type Outcome = {
  work_id: string
  edition_id: string
  title: string
  isbns: string[]
  duplicate: boolean
}

function shelved(title: string, shelfKey: string | null, shelfName: string | null): ShelvedBook {
  return {
    book_uri: `at://did:web:biblio/community.lexicon.book.edition/${title.replaceAll(' ', '-')}`,
    title,
    shelf_key: shelfKey,
    shelf_name: shelfName,
    shelved_at: '2026-10-24T23:54:31.707Z',
  }
}

/** Three books across two shelves, as the command returns them. */
function shelving(): ShelvedBook[] {
  return [
    shelved('Dune', 'read', 'Finished'),
    shelved('Dune Messiah', 'reading', 'Currently Reading'),
    shelved('Was Hinduism Invented?', null, null),
  ]
}

function outcome(title: string, duplicate = false): Outcome {
  return {
    work_id: '01hwwwwwwwwwwwwwwwwwwwwwww',
    edition_id: '01hzzzzzzzzzzzzzzzzzzzzzzz',
    title,
    isbns: [],
    duplicate,
  }
}

function mockInvoke(handlers: Record<string, (args?: unknown) => unknown>) {
  invoke.mockImplementation(async (command: string, args?: unknown) => {
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler(args)
  })
}

test('lists every shelved book with the shelf it sits on', async () => {
  mockInvoke({ list_shelved_books: () => shelving() })
  const { container } = await render(ShelvedBooksDrawerFixture)

  await expect.poll(() => container.textContent).toContain('Dune Messiah')
  expect(container.textContent).toContain('Finished')
  expect(container.textContent).toContain('Currently Reading')
  expect(container.textContent).toContain('Showing 3 of 3 books')
  expect(container.querySelectorAll('.result')).toHaveLength(3)
})

test('search and the shelf filter narrow the list', async () => {
  mockInvoke({ list_shelved_books: () => shelving() })
  const { container } = await render(ShelvedBooksDrawerFixture)
  await expect.poll(() => container.textContent).toContain('Dune Messiah')

  const search = container.querySelector('wa-input') as HTMLElement & { value: string }
  search.value = 'messiah'
  search.dispatchEvent(new Event('input', { bubbles: true }))

  await expect.poll(() => container.querySelectorAll('.result')).toHaveLength(1)
  expect(container.textContent).toContain('Dune Messiah')
  expect(container.textContent).not.toContain('Was Hinduism Invented?')
})

test('adding a book imports it and keeps the row from being added twice', async () => {
  mockInvoke({
    list_shelved_books: () => shelving(),
    import_shelved_book: () => outcome('Dune'),
  })
  const { container } = await render(ShelvedBooksDrawerFixture)
  await expect.poll(() => container.textContent).toContain('Dune Messiah')

  const addButtons = [...container.querySelectorAll('.result wa-button')]
  const duneRow = [...container.querySelectorAll('.result')].find(
    (row) => row.textContent?.includes('Dune') && !row.textContent?.includes('Messiah'),
  )
  expect(addButtons.length).toBeGreaterThan(0)
  duneRow?.querySelector('wa-button')?.click()

  await expect.poll(() => duneRow?.textContent).toContain('Added')
  expect(invoke).toHaveBeenCalledWith(
    'import_shelved_book',
    expect.objectContaining({ book: expect.objectContaining({ title: 'Dune' }) }),
  )
})

test('a book already in the library is reported as a duplicate', async () => {
  mockInvoke({
    list_shelved_books: () => [shelved('Dune', 'read', 'Finished')],
    import_shelved_book: () => outcome('Dune', true),
  })
  const { container } = await render(ShelvedBooksDrawerFixture)
  await expect.poll(() => container.textContent).toContain('Finished')

  container.querySelector('.result wa-button')?.click()

  await expect.poll(() => container.textContent).toContain('Already added')
})

test('an unread account shows the empty state', async () => {
  mockInvoke({ list_shelved_books: () => [] })
  const { container } = await render(ShelvedBooksDrawerFixture)

  await expect.poll(() => container.textContent).toContain('no shelved books yet')
})

test('a failed listing surfaces its message and offers a retry', async () => {
  mockInvoke({
    list_shelved_books: () => {
      throw { code: 'atproto', message: 'sign in to your ATProto account to import your shelves' }
    },
  })
  const { container } = await render(ShelvedBooksDrawerFixture)

  await expect
    .poll(() => container.textContent)
    .toContain('sign in to your ATProto account to import your shelves')
  expect(container.textContent).toContain('Try again')
})
