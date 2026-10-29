import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { EditionDetail } from '$lib/search'
import EditionDetailDrawerFixture from './fixtures/EditionDetailDrawerFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  // Each test installs its own reply; the signature is open so they can.
  invoke: vi.fn(async (_command: string): Promise<unknown> => []),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

function detail(overrides: Partial<EditionDetail> = {}): EditionDetail {
  return {
    id: '01hzzzzzzzzzzzzzzzzzzzzzzz',
    work_id: '01hwwwwwwwwwwwwwwwwwwwwwww',
    title: 'Positive Obsession',
    published_date: '2016-01-01',
    format: 'EPUB',
    language_code: 'en',
    language_name: 'English',
    notes: 'A note.',
    description: 'A description.',
    authors: [{ name: 'Susana M. Morris', role: 'aut', role_label: 'Author' }],
    publishers: ['Grove Press'],
    identifiers: [{ kind: 'isbn', value: '9780802124675' }],
    file: {
      file_path: '/books/positive-obsession.epub',
      file_format: 'epub',
      file_size_bytes: 1024,
      cover_path: null,
      blurhash: null,
      dominant_color: null,
      file_status: 'ok',
    },
    created_at: '2026-01-01',
    updated_at: null,
    ...overrides,
  }
}

beforeEach(() => {
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    throw new Error(`unexpected command: ${command}`)
  })
})

afterEach(() => {
  // wa-drawer escapes the render container like wa-dialog does.
  for (const drawer of document.querySelectorAll('wa-drawer')) drawer.remove()
})

test('renders the full record: title, authors, facts, identifiers, and file', async () => {
  const { container } = await render(EditionDetailDrawerFixture)

  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')
  expect(container.querySelector('.authors')?.textContent).toContain('Susana M. Morris')
  expect(container.textContent).toContain('EPUB')
  expect(container.textContent).toContain('Grove Press')
  expect(container.textContent).toContain('9780802124675')
  expect(container.textContent).toContain('A description.')
  expect(container.textContent).toContain('Read')
})

test('a missing file surfaces the error and offers re-linking', async () => {
  invoke.mockImplementation(async () => ({
    ...detail(),
    file: { ...detail().file, file_status: 'missing' },
  }))
  const { container } = await render(EditionDetailDrawerFixture)

  await expect.poll(() => container.textContent).toContain('moved or deleted')
  expect(container.textContent).toContain('Re-link file')
})

test('a null record reports no catalog record', async () => {
  invoke.mockImplementation(async () => null)
  const { container } = await render(EditionDetailDrawerFixture)

  await expect.poll(() => container.textContent).toContain('No catalog record found')
})

test('an audiobook with a file offers listening', async () => {
  invoke.mockImplementation(async () => ({
    ...detail(),
    format: 'audiobook',
    file: { ...detail().file, file_format: 'm4b' },
  }))
  const { container } = await render(EditionDetailDrawerFixture)

  await expect.poll(() => container.textContent).toContain('Listen')
})

test('the description is sanitized before it reaches the DOM', async () => {
  invoke.mockImplementation(async () => ({
    ...detail(),
    // The shape an untrusted EPUB or index record can carry.
    // Every payload tags `body.dataset`, so a surviving one is observable.
    description: `<p onclick="document.body.dataset.pwned = 'click'">Kept <strong>bold</strong></p><script>document.body.dataset.pwned = 'script'</script><img src="x" onerror="document.body.dataset.pwned = 'img'">`,
  }))
  const { container } = await render(EditionDetailDrawerFixture)

  const section = () =>
    [...container.querySelectorAll('section')].find(
      (s) => s.querySelector('h3')?.textContent === 'Description',
    )
  await expect.poll(() => section()?.textContent).toContain('Kept bold')
  expect(section()?.querySelector('strong')?.textContent).toBe('bold')
  expect(section()?.querySelector('script')).toBeNull()
  expect(section()?.querySelector('img')).toBeNull()
  expect(section()?.querySelector('[onclick]')).toBeNull()
  expect(document.body.dataset.pwned).toBeUndefined()
})
