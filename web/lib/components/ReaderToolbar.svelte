<script lang="ts">
import ActionButton from './ActionButton.svelte'

interface Props {
  /** Publication title; falls back to the edition id while loading. */
  title: string
  /** Whether the publication has finished loading. */
  ready: boolean
  /** Whether notes can be written yet (the saved list must be loaded first). */
  canAnnotate: boolean
  /** Whether the current page is already bookmarked. */
  bookmarked: boolean
  /** Whether bookmarks can be written yet (the saved list must be loaded first). */
  canBookmark: boolean
  onprevious: () => void
  onnext: () => void
  onannotate: () => void
  onbookmark: () => void
  onclose: () => void
}

let {
  title,
  ready,
  canAnnotate,
  bookmarked,
  canBookmark,
  onprevious,
  onnext,
  onannotate,
  onbookmark,
  onclose,
}: Props = $props()
</script>

<header class="bar">
  <h1 class="title">{title}</h1>
  <wa-button-group class="controls">
    <ActionButton onclick={onprevious} disabled={!ready} size="small">
      <wa-icon name="chevron-left"></wa-icon>
      Previous
    </ActionButton>
    <ActionButton onclick={onnext} disabled={!ready} size="small">
      Next
      <wa-icon name="chevron-right"></wa-icon>
    </ActionButton>
    <ActionButton onclick={onannotate} disabled={!ready || !canAnnotate} size="small">
      <wa-icon name="comment"></wa-icon>
      Annotate
    </ActionButton>
    <ActionButton
      onclick={onbookmark}
      disabled={!ready || !canBookmark || bookmarked}
      variant={bookmarked ? 'brand' : undefined}
      aria-pressed={bookmarked}
size="small"
    >
      <wa-icon name="bookmark"></wa-icon>
      {bookmarked ? 'Bookmarked' : 'Bookmark'}
    </ActionButton>
    <ActionButton onclick={onclose} size="small">
      Close
    </ActionButton>
  </wa-button-group>
</header>

<style>
  .bar {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-m);
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-m);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .controls {
    display: flex;
    gap: var(--wa-space-2xs);
  }
</style>
