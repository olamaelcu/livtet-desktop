import type { HighlightColor } from './highlights'
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

/**
 * Rewrite one note's text, and its colour when one is given.
 *
 * The anchor, id, and creation time survive: editing the words of a note must
 * not move its highlight or reorder the list. A colour of `undefined` keeps the
 * note's existing colour, so a reader who only typed new words does not lose
 * the highlight they picked when they wrote it.
 */
export function updateNoteText(
  notes: Annotation[],
  id: string,
  text: string,
  color?: HighlightColor,
): Annotation[] {
  return notes.map((note) =>
    note.id === id ? { ...note, text, color: color ?? note.color } : note,
  )
}
