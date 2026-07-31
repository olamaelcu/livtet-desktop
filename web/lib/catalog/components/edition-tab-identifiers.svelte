<script lang="ts">
import { commands, type IdentifierRow } from '$lib/bindings'

interface Props {
  editionId: string
}

let { editionId }: Props = $props()

let identifiers = $state<IdentifierRow[]>([])
let loading = $state(true)
let error = $state<string | null>(null)
let copiedId = $state<string | null>(null)

$effect(() => {
  let cancelled = false
  loading = true
  error = null
  identifiers = []
  commands
    .findIdentifiersByEdition(editionId)
    .then((res) => {
      if (cancelled) return
      if (res.status === 'ok') {
        identifiers = res.data
        error = null
      } else {
        error = res.error
        identifiers = []
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

async function copy(identifier: IdentifierRow): Promise<void> {
  try {
    await navigator.clipboard.writeText(identifier.value)
    copiedId = identifier.id
    setTimeout(() => {
      if (copiedId === identifier.id) copiedId = null
    }, 1500)
  } catch {
    // Clipboard unavailable (Tauri sandbox, etc.) — silently ignore.
  }
}
</script>

{#if loading}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="hourglass"></wa-icon>
    Loading identifiers…
  </wa-callout>
{:else if error}
  <wa-callout variant="danger">
    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
    Failed to load: {error}
  </wa-callout>
{:else if identifiers.length === 0}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="circle-info"></wa-icon>
    No identifiers linked to this edition.
  </wa-callout>
{:else}
  <ul class="identifiers">
    {#each identifiers as identifier (identifier.id)}
      <li>
        <span class="kind">{identifier.kind}</span>
        <code class="value">{identifier.value}</code>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <wa-button
          size="small"
          appearance="plain"
          role="button"
          tabindex="0"
          onclick={() => copy(identifier)}
          aria-label="Copy URN"
        >
          {#if copiedId === identifier.id}Copied{:else}Copy{/if}
        </wa-button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .identifiers {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  li {
    display: grid;
    grid-template-columns: max-content 1fr max-content;
    gap: 0.75rem;
    align-items: center;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 6px;
  }
  .kind {
    font-variant: small-caps;
    font-size: 0.8125rem;
    color: var(--wa-color-text-quiet, currentColor);
  }
  .value {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.8125rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>