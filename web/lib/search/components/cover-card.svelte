<script lang="ts">
  import type { SearchHit } from "../types";
  import { coverLetter, dominantColorFor } from "../mock-data";

  interface Props {
    hit: SearchHit;
  }

  let { hit }: Props = $props();

  const bg = $derived(dominantColorFor(hit.title));
  const letter = $derived(coverLetter(hit.title));
  const authorsLine = $derived(
    hit.authors.length === 0 ? "" : hit.authors.join(", "),
  );
</script>

<!-- TODO: promote to <button> (or <a>) when detail-on-click lands; that will
     remove the a11y_no_noninteractive_tabindex warning and give screen readers
     a real activation announcement. -->
<article
  class="cover-card"
  style:--dominant={bg}
  aria-label={hit.title}
  tabindex="0"
>
  <div class="cover" aria-hidden="true">
    <span class="cover-letter">{letter}</span>
  </div>
  <div class="cover-overlay">
    <div class="cover-title" aria-hidden="true">{hit.title}</div>
    {#if authorsLine}
      <div class="cover-authors">{authorsLine}</div>
    {/if}
  </div>
</article>

<style>
  .cover-card {
    display: block;
    position: relative;
    border-radius: var(--wa-border-radius-m, 6px);
    overflow: hidden;
    isolation: isolate;
    cursor: pointer;
    outline: none;
  }

  .cover-card:focus-visible {
    box-shadow: 0 0 0 2px var(--wa-color-focus, currentColor);
  }

  .cover {
    aspect-ratio: 2 / 3;
    background: var(--dominant);
    display: grid;
    place-items: center;
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.08),
      0 1px 3px rgba(0, 0, 0, 0.15);
  }

  .cover-letter {
    font-size: clamp(2.5rem, 8vw, 4.5rem);
    font-weight: 600;
    color: rgba(255, 255, 255, 0.92);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    font-family: var(--wa-font-family-heading, inherit);
  }

  .cover-overlay {
    position: absolute;
    inset: auto 0 0 0;
    padding: 0.75rem 0.875rem;
    color: white;
    background: linear-gradient(
      to top,
      rgba(0, 0, 0, 0.78),
      rgba(0, 0, 0, 0) 100%
    );
    opacity: 0;
    transition: opacity 150ms ease;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .cover-card:hover .cover-overlay,
  .cover-card:focus-visible .cover-overlay,
  .cover-card:focus-within .cover-overlay {
    opacity: 1;
  }

  .cover-title {
    font-size: 0.9375rem;
    font-weight: 600;
    line-height: 1.2;
  }

  .cover-authors {
    font-size: 0.8125rem;
    opacity: 0.85;
    line-height: 1.2;
  }
</style>