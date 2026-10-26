<script lang="ts">
import NotesPanel from '$lib/components/NotesPanel.svelte'
import type { HighlightColor } from '$lib/reader/highlights'
import type { Annotation } from '$lib/reader/progress'

interface Props {
  notes: Annotation[]
  events: string[]
}

let { notes, events }: Props = $props()
</script>

<NotesPanel
  {notes}
  onvisit={(note) => events.push(`visit:${note.text}`)}
  onedit={(note) => events.push(`edit:${note.text}`)}
  ondelete={(note) => events.push(`delete:${note.text}`)}
  onrecolor={(note, color: HighlightColor) => events.push(`color:${note.text}:${color}`)}
  onclose={() => events.push('close')}
/>
