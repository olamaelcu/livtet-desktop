<script lang="ts">
import AudioBookmarks from '$lib/components/AudioBookmarks.svelte'
import type { Bookmark } from '$lib/reader/bookmark'

interface Props {
  bookmarks?: Bookmark[]
  canAdd?: boolean
  bookmarked?: boolean
  events: string[]
}

let { bookmarks = [], canAdd = true, bookmarked = false, events }: Props = $props()
</script>

<AudioBookmarks
  {bookmarks}
  {canAdd}
  {bookmarked}
  onadd={() => events.push('add')}
  onvisit={(bookmark) => events.push(`visit:${bookmark.id}`)}
  onremove={(bookmark) => events.push(`remove:${bookmark.id}`)}
/>
