import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import ReaderToolbarFixture from './fixtures/ReaderToolbarFixture.svelte'

/** The bookmark control, found by its label rather than by position. */
function bookmarkButton(container: HTMLElement): HTMLElement {
  const button = [...container.querySelectorAll('wa-button')].find((candidate) =>
    candidate.textContent?.includes('Bookmark'),
  )
  if (!button) throw new Error('bookmark button is missing')
  return button as HTMLElement
}

test('bookmarks the page and reports the click', async () => {
  const events: string[] = []
  const { container } = await render(ReaderToolbarFixture, { props: { events } })

  const button = bookmarkButton(container)
  expect(button.textContent?.trim()).toBe('Bookmark')
  expect(button.hasAttribute('disabled')).toBe(false)
  expect(button.getAttribute('aria-pressed')).toBe('false')

  button.click()
  expect(events).toEqual(['bookmark'])
})

test('disables bookmarking once the page already carries a bookmark', async () => {
  const events: string[] = []
  const { container } = await render(ReaderToolbarFixture, {
    props: { bookmarked: true, events },
  })

  const button = bookmarkButton(container)
  expect(button.textContent?.trim()).toBe('Bookmarked')
  expect(button.hasAttribute('disabled')).toBe(true)
  expect(button.getAttribute('aria-pressed')).toBe('true')
})

test('stays disabled until the saved bookmarks have loaded', async () => {
  const { container } = await render(ReaderToolbarFixture, {
    props: { canBookmark: false, events: [] },
  })

  expect(bookmarkButton(container).hasAttribute('disabled')).toBe(true)
})
