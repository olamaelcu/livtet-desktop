import { render } from 'svelte/server'
import { describe, expect, test, vi } from 'vitest'

import ChapterList from '$lib/components/ChapterList.svelte'
import SelectionActionBar from '$lib/library/SelectionActionBar.svelte'

const chapters = [
  { name: 'One', audio_start: 0, audio_end: 60 },
  { name: 'Two', audio_start: 60, audio_end: 120 },
]

describe('SelectionActionBar', () => {
  const props = {
    count: 3,
    busy: false,
    ontag: vi.fn(),
    onexport: vi.fn(),
    ondelete: vi.fn(),
    onclear: vi.fn(),
    onselectall: vi.fn(),
  }

  test('announces the count and offers every action', () => {
    const { body } = render(SelectionActionBar, { props })

    expect(body).toContain('3 selected')
    expect(body).toContain('role="toolbar"')
    for (const label of ['Tag', 'Export CSV', 'Delete', 'Select all matching', 'Clear']) {
      expect(body).toContain(label)
    }
  })

  test('an empty selection disables the actions', () => {
    const { body } = render(SelectionActionBar, { props: { ...props, count: 0 } })

    expect(body).toContain('0 selected')
    expect(body).toContain('disabled')
  })
})

describe('ChapterList', () => {
  test('renders nothing without chapters', () => {
    const { body } = render(ChapterList, { props: { chapters: [] } })

    expect(body).not.toContain('Chapters')
  })

  test('lists chapters and marks the current one', () => {
    const { body } = render(ChapterList, {
      props: { chapters, currentPosition: 70, onChapterSelect: vi.fn() },
    })

    expect(body).toContain('Chapters')
    expect(body).toContain('One')
    expect(body).toContain('Two')
    expect(body).toContain('current')
  })
})
