<script lang="ts">
// Mirrors the pub reader's structure: its own header, then a growing viewport
// that carries data-scroll-region (Pane grows it; the EPUB iframe scrolls inside).
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import ReaderLayout from '../../../routes/reader/ReaderLayout.svelte'
</script>

<AppFrame>
  <ReaderLayout>
    <header>Title</header>
    <div class="viewport" data-scroll-region>
      <div style="height: 3000px">book</div>
    </div>
  </ReaderLayout>
</AppFrame>

<style>
  .viewport {
    position: relative;
    min-width: 0;
    overflow: hidden; /* stands in for the iframe containing its own content */
  }
</style>
