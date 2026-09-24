<script lang="ts">
interface Props {
  title: string
  cover_url?: string
  onclick?: () => void
}

let { title, cover_url = undefined, onclick }: Props = $props()
</script>

<style>
  .card {
    all: unset;
    cursor: pointer;
    transition: box-shadow 0.15s ease;
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

<button class="card" type="button" {onclick}>
  <figure class="cover {cover_url ? '' : 'placeholder'}">
    {#if cover_url}
      <img src={cover_url} alt={title} />
    {:else}
      <figcaption>{title}</figcaption>
    {/if}
  </figure>
</button>
