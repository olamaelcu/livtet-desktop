import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { EditionDetail } from '$lib/search'
import EditionDetailDrawerFixture from './fixtures/EditionDetailDrawerFixture.svelte'

const { convertFileSrc, invoke, openUrl } = vi.hoisted(() => ({
  openUrl: vi.fn(async (_url: string) => {}),
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  // Typed so `mock.calls` keeps its `[command, args]` tuple shape.
  invoke: vi.fn(async (_command: string, _args?: Record<string, unknown>): Promise<unknown> => []),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl, revealItemInDir: vi.fn(async () => {}) }))

const EDITION_ID = '01hzzzzzzzzzzzzzzzzzzzzzzz'
const REVIEWS = [
  {
    id: 'review-2',
    edition_id: EDITION_ID,
    text: 'Worth the reread.',
    rating: 8.5,
    created_at: '2026-10-28T10:00:00Z',
    record_uri: 'at://did:plc:x/net.olamaelcu.livtet.bookReview/3l3qo2vqk2s2a',
  },
  {
    id: 'review-1',
    edition_id: EDITION_ID,
    text: 'No stars from me.',
    rating: null,
    created_at: '2026-10-27T10:00:00Z',
    record_uri: null,
  },
]
const SIGNED_IN = { did: 'did:plc:e2ctbutx6kya6si4if5ngjmm', scope: 'atproto', expires_at: null }
const BOOK_PAGE_URL =
  'https://livtet.olamaelcu.net/book/did:web:biblio.livtet.olamaelcu.net/ol.OL26224505M'

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
    description: 'A description.',
    authors: [{ name: 'Susana M. Morris', role: 'aut', role_label: 'Author' }],
    publishers: ['Grove Press'],
    identifiers: [],
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
  openUrl.mockReset()
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'save_local_review') {
      return {
        id: 'review-3',
        edition_id: EDITION_ID,
        text: 'great',
        rating: 8,
        created_at: '2026-01-01T00:00:00Z',
        record_uri: null,
      }
    }
    if (command === 'remove_local_review') return true
    if (command === 'load_local_reviews') return REVIEWS
    if (command === 'atproto_status') return SIGNED_IN
    if (command === 'put_local_review_to_pds') {
      return {
        record_uri: 'at://did:plc:x/net.olamaelcu.livtet.bookReview/3l3qo2vqk2s2a',
        book_page_url: BOOK_PAGE_URL,
        error: null,
      }
    }
    return []
  })
})

afterEach(() => {
  // wa-drawer and wa-dialog escape the render container.
  for (const portal of document.querySelectorAll('wa-drawer, wa-dialog')) portal.remove()
})

async function openReviewDialog() {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')

  const trigger = container.querySelector('#review-section wa-button')
  expect(trigger?.textContent).toContain('Write Review')
  ;(trigger as HTMLElement).click()

  const dialog = await vi.waitFor(() => {
    const found = document.querySelector('wa-dialog')
    if (!found) throw new Error('review dialog did not open')
    return found
  })
  return { container, dialog: dialog as HTMLElement }
}

/**
 * Fill a Web Awesome form control and fire the event its listener reads.
 *
 * The components are declared in `app.wa.js`, which the browser project does
 * not load, so the elements are plain custom elements here: `value` is a
 * plain own property and the Svelte listener is the only reader.
 */
function setControl(root: ParentNode, selector: string, value: string | number) {
  const control = root.querySelector(selector) as (HTMLElement & { value: string | number }) | null
  if (!control) throw new Error(`missing ${selector}`)
  control.value = value
  control.dispatchEvent(new Event('input', { bubbles: true }))
  return control
}

/**
 * Pick a `wa-rating` value. The real component emits `change` (never `input`)
 * when its value changes, so the form listener only sees a `change` event.
 */
function setRating(root: ParentNode, value: number) {
  const control = root.querySelector('wa-rating') as (HTMLElement & { value: number }) | null
  if (!control) throw new Error('missing wa-rating')
  control.value = value
  control.dispatchEvent(new Event('change', { bubbles: true }))
  return control
}

/**
 * Submit once the form's reactive state has seen the typed value.
 *
 * These are plain custom elements in the browser project (no `app.wa.js`), so
 * Svelte writes boolean attributes as strings: `disabled="false"` is enabled.
 */
async function submitWhenReady(dialog: HTMLElement) {
  const submit = dialog.querySelector('wa-button[type="submit"]')
  await vi.waitFor(() => expect(submit?.getAttribute('disabled')).toBe('false'))
  dialog
    .querySelector('form')
    ?.dispatchEvent(new Event('submit', { cancelable: true, bubbles: true }))
}

test('the drawer offers a review dialog with text, a rating, and a publish switch', async () => {
  const { dialog } = await openReviewDialog()

  expect(dialog.querySelector('wa-textarea')).not.toBeNull()
  expect(dialog.querySelector('wa-rating')).not.toBeNull()
  expect(dialog.textContent).toContain('Submit')

  const toggle = dialog.querySelector('wa-switch')
  expect(toggle).not.toBeNull()
  await vi.waitFor(() => expect(toggle?.getAttribute('checked')).toBe('true'))
  expect(toggle?.getAttribute('disabled')).not.toBe('true')
})

test('submitting saves the review locally and publishes it to the PDS', async () => {
  const { dialog } = await openReviewDialog()

  setControl(dialog, 'wa-textarea', 'Slow start, great ending.')
  setRating(dialog, 8)
  await submitWhenReady(dialog)

  await vi.waitFor(() => {
    const commands = invoke.mock.calls.map(([command]) => command)
    expect(commands).toContain('save_local_review')
    expect(commands).toContain('put_local_review_to_pds')
  })

  const saved = invoke.mock.calls.find(([command]) => command === 'save_local_review')
  expect(saved?.[1]).toEqual({
    editionId: EDITION_ID,
    text: 'Slow start, great ending.',
    rating: 8,
  })
  const published = invoke.mock.calls.find(([command]) => command === 'put_local_review_to_pds')
  expect(published?.[1]).toEqual({
    reviewId: 'review-3',
    title: 'Positive Obsession',
  })

  // The dialog is dismissed once the review is stored.
  await expect
    .poll(() => document.querySelector('wa-dialog')?.getAttribute('open'))
    .not.toBe('true')
})

test('a published review offers the book page on Livtet', async () => {
  const { container, dialog } = await openReviewDialog()

  setControl(dialog, 'wa-textarea', 'Worth the reread.')
  await submitWhenReady(dialog)

  const link = await vi.waitFor(() => {
    const buttons = [...container.querySelectorAll('#review-section .review-actions wa-button')]
    const found = buttons.find((button) => button.textContent?.includes('View on Livtet'))
    if (!found) throw new Error('no Livtet link yet')
    return found as HTMLElement
  })
  link.click()
  await vi.waitFor(() => expect(openUrl).toHaveBeenCalledWith(BOOK_PAGE_URL))
})

test('the Reviews button opens a drawer listing the saved reviews', async () => {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')

  const trigger = await vi.waitFor(() => {
    const buttons = [...container.querySelectorAll('#review-section .review-actions wa-button')]
    const found = buttons.find((button) => button.textContent?.includes('Reviews'))
    if (!found) throw new Error('no Reviews button')
    return found as HTMLElement
  })
  // The count is known before the listing is opened, so the button can show it.
  await vi.waitFor(() => expect(trigger.textContent).toContain('(2)'))
  trigger.click()

  const drawer = await vi.waitFor(() => {
    const found = document.querySelector('wa-drawer[label="Reviews"]')
    if (!found) throw new Error('reviews drawer did not open')
    return found as HTMLElement
  })

  await vi.waitFor(() => expect(drawer.textContent).toContain('Worth the reread.'))
  const rows = drawer.querySelectorAll('.review')
  expect(rows).toHaveLength(2)
  expect(rows[0]?.textContent).toContain('8.5/10')
  expect(rows[1]?.textContent).toContain('No stars from me.')
  expect(rows[1]?.querySelector('.review-rating')).toBeNull()
})

test('editing a review prefills the form and republishes the same record', async () => {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')

  const reviewsTrigger = await vi.waitFor(() => {
    const buttons = [...container.querySelectorAll('#review-section .review-actions wa-button')]
    const found = buttons.find((button) => button.textContent?.includes('Reviews'))
    if (!found) throw new Error('no Reviews button')
    return found as HTMLElement
  })
  reviewsTrigger.click()

  const drawer = await vi.waitFor(() => {
    const found = document.querySelector('wa-drawer[label="Reviews"]')
    if (!found) throw new Error('reviews drawer did not open')
    return found as HTMLElement
  })
  await vi.waitFor(() => expect(drawer.querySelectorAll('.review')).toHaveLength(2))

  const editButton = drawer.querySelector('.review-edit')
  if (!(editButton instanceof HTMLElement)) throw new Error('no edit button')
  editButton.click()

  const dialog = await vi.waitFor(() => {
    const found = document.querySelector('wa-dialog[label="Edit review"]')
    if (!found) throw new Error('edit dialog did not open')
    return found as HTMLElement
  })

  const textarea = dialog.querySelector('wa-textarea') as (HTMLElement & { value: string }) | null
  const rating = dialog.querySelector('wa-rating') as (HTMLElement & { value: number }) | null
  // The harness elements are plain, so Svelte writes `value` as an attribute.
  await vi.waitFor(() => expect(textarea?.getAttribute('value')).toBe(REVIEWS[0]?.text))
  expect(rating?.getAttribute('value')).toBe(String(REVIEWS[0]?.rating))

  setControl(dialog, 'wa-textarea', 'A revised take.')
  setRating(dialog, 7)
  await submitWhenReady(dialog)

  await vi.waitFor(() => {
    const commands = invoke.mock.calls.map(([command]) => command)
    expect(commands).toContain('save_local_review')
    expect(commands).toContain('put_local_review_to_pds')
  })
  const saved = invoke.mock.calls.find(([command]) => command === 'save_local_review')
  expect(saved?.[1]).toEqual({ editionId: EDITION_ID, text: 'A revised take.', rating: 7 })
  const published = invoke.mock.calls.find(([command]) => command === 'put_local_review_to_pds')
  expect(published?.[1]).toEqual({ reviewId: 'review-3', title: 'Positive Obsession' })
})

test('an empty review list says so', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'load_local_reviews') return []
    return []
  })

  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')

  const buttons = [...container.querySelectorAll('#review-section .review-actions wa-button')]
  const trigger = buttons.find((button) => button.textContent?.includes('Reviews')) as
    | HTMLElement
    | undefined
  expect(trigger).toBeDefined()
  expect(trigger?.textContent).toContain('Reviews')
  trigger?.click()

  const drawer = await vi.waitFor(() => {
    const found = document.querySelector('wa-drawer[label="Reviews"]')
    if (!found) throw new Error('reviews drawer did not open')
    return found as HTMLElement
  })
  await vi.waitFor(() =>
    expect(drawer.textContent).toContain('No reviews saved for this book yet.'),
  )
})

test('a published review is marked, and deleting one leaves the PDS copy alone', async () => {
  let saved = [...REVIEWS]
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'load_local_reviews') return saved
    if (command === 'remove_local_review') {
      const last = invoke.mock.calls[invoke.mock.calls.length - 1]
      const args = last?.[1] as { reviewId?: string } | undefined
      saved = saved.filter((review) => review.id !== args?.reviewId)
      return true
    }
    return []
  })

  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent)
    .toBe('Positive Obsession')

  const buttons = [...container.querySelectorAll('#review-section .review-actions wa-button')]
  ;(buttons.find((b) => b.textContent?.includes('Reviews')) as HTMLElement).click()

  const drawer = await vi.waitFor(() => {
    const found = document.querySelector('wa-drawer[label="Reviews"]')
    if (!found) throw new Error('reviews drawer did not open')
    return found as HTMLElement
  })
  await vi.waitFor(() => expect(drawer.querySelectorAll('.review')).toHaveLength(2))

  const rows = [...drawer.querySelectorAll('.review')]
  expect(rows[0]?.querySelector('wa-badge')?.textContent).toContain('Published')
  expect(rows[1]?.querySelector('wa-badge')).toBeNull()

  const deleteButton = rows[0]?.querySelector('.review-delete')
  if (!(deleteButton instanceof HTMLElement)) throw new Error('no delete button on the first row')
  deleteButton.click()

  await vi.waitFor(() => {
    const called = invoke.mock.calls.find(([command]) => command === 'remove_local_review')
    expect(called?.[1]).toEqual({ reviewId: 'review-2' })
  })
  await vi.waitFor(() => expect(drawer.querySelectorAll('.review')).toHaveLength(1))
  expect(drawer.textContent).not.toContain('Worth the reread.')
})

test('turning the switch off keeps the review local', async () => {
  const { dialog } = await openReviewDialog()

  setControl(dialog, 'wa-textarea', 'Just for me.')
  const toggle = dialog.querySelector('wa-switch') as HTMLElement & { checked: boolean }
  toggle.checked = false
  toggle.dispatchEvent(new Event('change', { bubbles: true }))
  await vi.waitFor(() => expect(toggle.getAttribute('checked')).toBe('false'))

  await submitWhenReady(dialog)

  await vi.waitFor(() => {
    expect(invoke.mock.calls.map(([command]) => command)).toContain('save_local_review')
  })
  expect(invoke.mock.calls.map(([command]) => command)).not.toContain('put_local_review_to_pds')
  expect(document.querySelector('wa-dialog')).not.toBeNull()
})

test('a signed-out account cannot turn publishing on', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'atproto_status') return null
    return []
  })

  const { dialog } = await openReviewDialog()
  const toggle = dialog.querySelector('wa-switch')
  await vi.waitFor(() => expect(toggle?.getAttribute('disabled')).toBe('true'))
  expect(toggle?.getAttribute('checked')).not.toBe('true')
  expect(dialog.textContent).toContain('Sign in on Settings')

  setControl(dialog, 'wa-textarea', 'Offline thoughts.')
  await submitWhenReady(dialog)
  await vi.waitFor(() => {
    expect(invoke.mock.calls.map(([command]) => command)).toContain('save_local_review')
  })
  expect(invoke.mock.calls.map(([command]) => command)).not.toContain('put_local_review_to_pds')
})

test('an unrated review sends a null rating', async () => {
  const { dialog } = await openReviewDialog()

  setControl(dialog, 'wa-textarea', 'No stars from me.')
  await submitWhenReady(dialog)

  await vi.waitFor(() => {
    expect(invoke.mock.calls.map(([command]) => command)).toContain('save_local_review')
  })
  const saved = invoke.mock.calls.find(([command]) => command === 'save_local_review')
  expect(saved?.[1]).toMatchObject({ rating: null })
})

test('a save failure keeps the dialog open and shows the reason', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'get_edition_detail') return detail()
    if (command === 'save_local_review') {
      // `typedError` turns a non-Error rejection into `{ status: 'error' }`.
      throw { code: 'database', message: 'disk is full' }
    }
    return []
  })

  const { dialog } = await openReviewDialog()
  setControl(dialog, 'wa-textarea', 'Words')
  await submitWhenReady(dialog)

  await vi.waitFor(() => {
    expect(dialog.textContent).toContain('disk is full')
  })
  expect(document.querySelector('wa-dialog')).not.toBeNull()
})
