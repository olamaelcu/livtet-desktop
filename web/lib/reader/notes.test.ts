import { describe, expect, it } from 'vitest'
import { filterNotes, updateNoteText } from './notes'
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

describe('updateNoteText', () => {
  const target = note({
    id: 'a',
    text: 'first take',
    quote: 'Octavia took the bus',
    locator: '{"href":"/ch1"}',
    color: 'yellow',
    createdAt: '2026-10-26T05:57:38.074Z',
  })

  it('keeps the anchor, id, and creation time of the edited note', () => {
    const [edited] = updateNoteText([target], 'a', 'second take')
    expect(edited).toEqual({ ...target, text: 'second take' })
  })

  // Only new words reach the composer, so the colour has to survive on its own.
  it('keeps the existing colour when no new one is given', () => {
    const [edited] = updateNoteText([target], 'a', 'second take')
    expect(edited?.color).toBe('yellow')
  })

  it('takes a new colour alongside the text', () => {
    const [edited] = updateNoteText([target], 'a', 'second take', 'blue')
    expect(edited?.color).toBe('blue')
  })

  it('leaves every other note untouched', () => {
    const other = note({ id: 'b', text: 'untouched' })
    const updated = updateNoteText([target, other], 'a', 'second take')
    expect(updated[1]).toBe(other)
  })
})
