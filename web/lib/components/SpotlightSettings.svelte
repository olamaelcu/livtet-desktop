<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { spotlightKeys } from '../query/keys'
import {
  getSpotlightStatus,
  reindexSpotlight,
  setSpotlightEnabled,
  spotlightErrorMessage,
} from '../spotlight'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const status = createQuery(() => ({
  queryKey: spotlightKeys.status(),
  queryFn: getSpotlightStatus,
}))

let enabled = $state(false)
let switchControl = $state<WaSwitchElement>()

// Plain (not `$state`) so it is not an effect dependency: the switch takes the
// backend's value once, then belongs to the user.
let seeded = false
$effect(() => {
  const current = status.data
  if (seeded || !current) return
  enabled = current.enabled
  seeded = true
})

const toggle = createMutation(() => ({
  mutationFn: (next: boolean) => setSpotlightEnabled(next),
  onSuccess: (next) => {
    queryClient.setQueryData(spotlightKeys.status(), next)
    enabled = next.enabled
    seeded = true
    toast.success(
      next.enabled ? `Indexed ${next.item_count} editions for Spotlight` : 'Spotlight indexing off',
    )
  },
  onError: (error: unknown) => {
    // The backend refused: put the switch back rather than claim a state it
    // does not have. wa-switch owns its own `checked` once clicked.
    const previous = status.data?.enabled ?? false
    enabled = previous
    if (switchControl) switchControl.checked = previous
    toast.error(spotlightErrorMessage(error))
  },
}))

const rebuild = createMutation(() => ({
  mutationFn: reindexSpotlight,
  onSuccess: (next) => {
    queryClient.setQueryData(spotlightKeys.status(), next)
    toast.success(`Indexed ${next.item_count} editions for Spotlight`)
  },
  onError: (error: unknown) => toast.error(spotlightErrorMessage(error)),
}))

function onToggle(event: Event) {
  enabled = (event.currentTarget as WaSwitchElement).checked
  toggle.mutate(enabled)
}

function indexedAt(value: string | null): string {
  if (!value) return 'never'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}
</script>

<section class="spotlight" aria-labelledby="spotlight-heading">
  <h2 id="spotlight-heading">Spotlight</h2>
  <p class="description">
    Adds the library to macOS Spotlight so books are searchable with
    <kbd>⌘</kbd><kbd>Space</kbd>. Choosing a result opens that edition in
    Livtet. Indexing is local to this Mac and can be turned off at any time,
    which removes Livtet's items from the index.
  </p>

  {#if status.isPending}
    <p class="muted">Loading…</p>
  {:else if status.isError}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      {spotlightErrorMessage(status.error)}
    </wa-callout>
  {:else if !status.data}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      The backend returned no Spotlight settings.
    </wa-callout>
  {:else if !status.data.available}
    <wa-callout variant="neutral">
      <wa-icon slot="icon" name="circle-info"></wa-icon>
      Spotlight indexing is only available on macOS. Signed builds made from this
      source can use it; a development build may not appear in Spotlight.
    </wa-callout>
  {:else}
    <wa-callout variant={status.data.enabled ? 'success' : 'neutral'}>
      <wa-icon
        slot="icon"
        name={status.data.enabled ? 'circle-check' : 'circle-xmark'}
      ></wa-icon>
      {#if status.data.enabled}
        {status.data.item_count} editions indexed; last refreshed {indexedAt(status.data.indexed_at)}.
      {:else}
        Not indexed.
      {/if}
    </wa-callout>

    <div class="controls">
      <wa-switch
        bind:this={switchControl}
        aria-label="Index the library in Spotlight"
        checked={enabled}
        disabled={toggle.isPending}
        onchange={onToggle}
      >
        Index the library in Spotlight
      </wa-switch>
      <ActionButton
        onclick={() => rebuild.mutate()}
        disabled={!enabled || rebuild.isPending || toggle.isPending}
      >
        {rebuild.isPending ? 'Rebuilding…' : 'Rebuild index'}
      </ActionButton>
    </div>
  {/if}
</section>

<style>
  .spotlight {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .spotlight h2 {
    margin: 0;
  }

  .description {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }

  .muted {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: var(--wa-space-m);
    flex-wrap: wrap;
  }

  kbd {
    font-family: var(--wa-font-family-mono, monospace);
    border: 1px solid var(--wa-color-surface-border);
    border-radius: var(--wa-radius-xs);
    padding: 0 var(--wa-space-2xs);
  }
</style>
