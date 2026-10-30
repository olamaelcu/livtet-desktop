import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { Bookmark } from '$lib/reader/bookmark'
import AudioBookmarksFixture from './fixtures/AudioBookmarksFixture.svelte'

const spot = (id: string, positionSeconds: number): Bookmark => ({
  id,
  kind: 'timestamp',
  positionSeconds,
})

/** The Add control is the first `wa-button` in the section. */
function addButton(container: HTMLElement): HTMLElement & { disabled: boolean } {
  const button = container.querySelector('wa-button')
  expect(button, 'the section renders an Add button').not.toBeNull()
  return button as HTMLElement & { disabled: boolean }
}

function rows(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll('.bookmark')] as HTMLElement[]
}

test('adds the spot on screen', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, { props: { events } })

  await expect.poll(() => addButton(container).disabled).toBe(false)
  addButton(container).click()

  expect(events).toEqual(['add'])
})

test('labels the button by whether the spot is already bookmarked', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, {
    props: { events, bookmarked: true },
  })

  expect(addButton(container).textContent).toContain('Bookmarked')
  expect(addButton(container).getAttribute('aria-pressed')).toBe('true')
  // Adding is one-way from here: the list owns removal.
  await expect.poll(() => addButton(container).disabled).toBe(true)
})

// The stored list, and any save already in flight, gate the button: a second
// click before the first read completes would otherwise stack duplicates.
test('refuses to add while the reader cannot save', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, {
    props: { events, canAdd: false },
  })

  await expect.poll(() => addButton(container).disabled).toBe(true)
  expect(addButton(container).textContent).toContain('Bookmark this spot')
})

test('lists each spot as a clock position, oldest first', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, {
    props: { events, bookmarks: [spot('a', 2527), spot('b', 65)] },
  })

  expect(rows(container)).toHaveLength(2)
  expect(container.querySelector('.section-title')?.textContent).toContain('Bookmarks (2)')
  expect(rows(container)[0]?.querySelector('.bookmark-jump')?.textContent?.trim()).toBe('0:42:07')
  expect(rows(container)[1]?.querySelector('.bookmark-jump')?.textContent?.trim()).toBe('0:01:05')
})

test('reports the spot a reader jumps to', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, {
    props: { events, bookmarks: [spot('a', 2527)] },
  })

  rows(container)[0]?.querySelector<HTMLButtonElement>('.bookmark-jump')?.click()

  expect(events).toEqual(['visit:a'])
})

test('reports the spot a reader removes, without touching playback', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, {
    props: { events, bookmarks: [spot('a', 2527)] },
  })

  const remove = [...(rows(container)[0]?.querySelectorAll('wa-button') ?? [])][0] as HTMLElement
  remove.click()

  expect(events).toEqual(['remove:a'])
})

test('renders no list before any spot is bookmarked', async () => {
  const events: string[] = []
  const { container } = await render(AudioBookmarksFixture, { props: { events } })

  expect(rows(container)).toHaveLength(0)
  expect(container.querySelector('.section-title')).toBeNull()
})
