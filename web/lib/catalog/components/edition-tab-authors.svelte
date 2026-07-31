<script lang="ts">
import { type AuthorWithRole, commands } from '$lib/bindings'

interface Props {
  editionId: string
}

let { editionId }: Props = $props()

let authors = $state<AuthorWithRole[]>([])
let loading = $state(true)
let error = $state<string | null>(null)

$effect(() => {
  let cancelled = false
  loading = true
  error = null
  authors = []
  commands
    .findAuthorsByEdition(editionId)
    .then((res) => {
      if (cancelled) return
      if (res.status === 'ok') {
        authors = res.data
        error = null
      } else {
        error = res.error
        authors = []
      }
      loading = false
    })
    .catch((e: unknown) => {
      if (cancelled) return
      error = String(e)
      loading = false
    })
  return () => {
    cancelled = true
  }
})
</script>

{#if loading}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="hourglass"></wa-icon>
    Loading authors…
  </wa-callout>
{:else if error}
  <wa-callout variant="danger">
    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
    Failed to load: {error}
  </wa-callout>
{:else if authors.length === 0}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="circle-info"></wa-icon>
    No authors linked to this edition.
  </wa-callout>
{:else}
  <ul class="authors">
    {#each authors as author (author.id + ":" + author.role)}
      <li>
        <span class="name">{author.name}</span>
        <span class="role">{author.role}</span>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .authors {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  li {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 6px;
  }
  .name {
    font-weight: 600;
  }
  .role {
    font-size: 0.8125rem;
    color: var(--wa-color-text-quiet, currentColor);
    font-variant: small-caps;
  }
</style>