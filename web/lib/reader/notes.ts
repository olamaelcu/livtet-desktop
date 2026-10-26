import type { Annotation } from './progress'

/**
 * Notes matching a free-text query, in their original order.
 *
 * Every whitespace-separated term must appear in the note's text or in the
 * passage it quotes, case-insensitively — so "octavia job" narrows to notes
 * mentioning both rather than either. An empty query matches everything.
 */
export function filterNotes(notes: Annotation[], query: string): Annotation[] {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (terms.length === 0) return notes
  return notes.filter((note) => {
    const haystack = `${note.text} ${note.quote ?? ''}`.toLowerCase()
    return terms.every((term) => haystack.includes(term))
  })
}
