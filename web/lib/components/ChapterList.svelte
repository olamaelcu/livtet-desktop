<script lang="ts">
  import type { ReaderChapter } from '../bindings';
  import { chapterAt } from '../reader';
  
  export let chapters: ReaderChapter[] = [];
  export let currentPosition: number = 0;
  export let onChapterSelect: (startPosition: number) => void = () => {};
  
  $: current = chapterAt(chapters, currentPosition);
</script>

{#if chapters.length > 0}
  <section class="section">
    <h3 class="section-title">Chapters</h3>
    <table class="chapters-table">
      <thead>
        <tr>
          <th class="th-index">#</th>
          <th class="th-title">Chapter</th>
        </tr>
      </thead>
      <tbody>
        {#each chapters as chapter, idx (idx)}
          <tr
            class:current={idx === current}
            onclick={() => onChapterSelect(chapter.audio_start)}
          >
            <td>{chapter.audio_start}</td>
            <td>
              <span class="chapter-name-wrap">
                {chapter.name}
              </span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .section-title {
    margin: var(--wa-space-s) 0;
    font-size: 1rem;
  }

  .chapters-table {
    width: 100%;
    border-collapse: collapse;
    margin-top: var(--wa-space-s);
  }

  .chapters-table th,
  .chapters-table td {
    padding: var(--wa-space-xxs) var(--wa-space-s);
    border: 1px solid var(--wa-color-divider);
  }

  .chapters-table th {
    font-size: 0.75rem;
    color: var(--wa-color-text-muted);
    text-align: left;
    background: var(--wa-color-bg-subtle);
  }

  .chapters-table td {
    font-size: 0.875rem;
  }

  .chapters-table tr {
    cursor: pointer;
  }

  .chapters-table tr:hover {
    background: var(--wa-color-bg-hover);
  }

  .chapters-table tr.current {
    background: var(--wa-color-brand-fill-quiet);
  }

  .chapter-name-wrap {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 30ch;
    margin: 0 auto;
  }

  .th-index { width: 6rem; }
</style>