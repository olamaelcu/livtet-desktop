<script lang="ts">
interface Author {
  name: string
  role: string
}

export const id: string = ''
export let title: string = ''
export let authors: Author[] = []
export let published: string = ''
export let description: string = ''
export let cover_url: string | undefined = undefined
export let popoverId: string = ''
</script>

<style>
  .card {
    display: flex;
    gap: var(--wa-space-l);
    padding: var(--wa-space-xl);
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-default);
    cursor: pointer;
    transition: box-shadow 0.15s ease;
    width: 100%;
    all: unset;
  }

  .card:hover,
  .card:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .cover {
    --size: 8.25rem;
    width: 100%;
    min-width: var(--size);
    height: var(--size);
    flex-shrink: 0;
    border-radius: calc(var(--wa-border-radius) - 0.125rem);
    overflow: hidden;
    border: 0.0625rem solid var(--wa-color-border-default);
    background: var(--wa-color-surface-alt);
    margin: 0;
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .placeholder {
    background: linear-gradient(135deg, #2d323f 0%, #3a3f4d 100%);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.65rem;
    color: var(--wa-color-text-secondary);
    text-align: center;
  }
</style>

<button class="card" type="button" id={popoverId}>
  <figure class="cover {cover_url ? '' : 'placeholder'}">
    {#if cover_url}
      <img src={cover_url} alt={title} />
    {:else}
      <figcaption>Book</figcaption>
    {/if}
  </figure>
 </button>
