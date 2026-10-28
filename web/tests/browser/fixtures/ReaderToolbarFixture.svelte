<script lang="ts">
import ReaderToolbar from '$lib/components/ReaderToolbar.svelte'

interface Props {
  bookmarked?: boolean
  canBookmark?: boolean
  events: string[]
}

let { bookmarked = false, canBookmark = true, events }: Props = $props()
</script>

<ReaderToolbar
  title="A Book"
  ready
  canAnnotate
  {bookmarked}
  {canBookmark}
  onprevious={() => events.push('previous')}
  onnext={() => events.push('next')}
  onannotate={() => events.push('annotate')}
  onbookmark={() => events.push('bookmark')}
  onclose={() => events.push('close')}
/>
