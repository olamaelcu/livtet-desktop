<script lang="ts">
import NotesPanel from '$lib/components/NotesPanel.svelte'
import type { Bookmark } from '$lib/reader/bookmark'
import type { HighlightColor } from '$lib/reader/highlights'
import type { Annotation } from '$lib/reader/progress'

interface Props {
  notes: Annotation[]
  events: string[]
  /** Bookmarks listed above the notes; empty by default so note tests stay about notes. */
  bookmarks?: Bookmark[]
}

let { notes, events, bookmarks = [] }: Props = $props()
</script>

<NotesPanel
  {notes}
  {bookmarks}
  onvisit={(note) => events.push(`visit:${note.text}`)}
  onedit={(note) => events.push(`edit:${note.text}`)}
  ondelete={(note) => events.push(`delete:${note.text}`)}
  onrecolor={(note, color: HighlightColor) => events.push(`color:${note.text}:${color}`)}
  onvisitbookmark={(bookmark) => events.push(`visitbookmark:${bookmark.id}`)}
  onremovebookmark={(bookmark) => events.push(`removebookmark:${bookmark.id}`)}
  onclose={() => events.push('close')}
/>
