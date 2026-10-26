import { describe, expect, it } from 'vitest'
import { filterNotes } from './notes'
import type { Annotation } from './progress'

const note = (overrides: Partial<Annotation> & { id: string }): Annotation => ({
  text: 'note text',
  ...overrides,
})

const notes: Annotation[] = [
  note({ id: 'a', text: 'Interesting take', quote: 'Octavia took the bus to the mailing house' }),
  note({ id: 'b', text: 'the job crushed her', quote: 'a life of drudgery' }),
  note({ id: 'c', text: 'Pattern Master chapter' }),
]

const ids = (query: string) => filterNotes(notes, query).map((item) => item.id)

describe('filterNotes', () => {
  it('matches everything on an empty or blank query', () => {
    expect(ids('')).toEqual(['a', 'b', 'c'])
    expect(ids('   ')).toEqual(['a', 'b', 'c'])
  })

  it('matches the note text case-insensitively', () => {
    expect(ids('INTERESTING')).toEqual(['a'])
  })

  it('matches the quoted passage, not only the note text', () => {
    expect(ids('drudgery')).toEqual(['b'])
  })

  it('requires every term, so extra words narrow the result', () => {
    expect(ids('octavia bus')).toEqual(['a'])
    expect(ids('octavia drudgery')).toEqual([])
  })

  it('returns nothing when no note matches', () => {
    expect(ids('zebra')).toEqual([])
  })

  it('keeps the original order', () => {
    // "the" appears in a (quote) and b (text), but not in c.
    expect(ids('the')).toEqual(['a', 'b'])
  })
})
