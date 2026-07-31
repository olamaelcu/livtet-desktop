<script lang="ts">
import { commands, type EditionRow } from '$lib/bindings'

interface Props {
  editionId: string
}

let { editionId }: Props = $props()

let row = $state<EditionRow | null>(null)
let loading = $state(true)
let error = $state<string | null>(null)

$effect(() => {
  let cancelled = false
  loading = true
  error = null
  row = null
  commands
    .findEditionById(editionId)
    .then((res) => {
      if (cancelled) return
      if (res.status === 'ok') {
        row = res.data
        error = null
      } else {
        error = res.error
        row = null
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
    Loading edition…
  </wa-callout>
{:else if error}
  <wa-callout variant="danger">
    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
    Failed to load: {error}
  </wa-callout>
{:else if !row}
  <wa-callout variant="warning">
    <wa-icon slot="icon" name="circle-info"></wa-icon>
    No edition with id {editionId}.
  </wa-callout>
{:else}
  <dl class="edition-fields">
    {#if row.title}
      <dt>Title</dt>
      <dd>{row.title}</dd>
    {/if}
    <dt>Work id</dt>
    <dd><code>{row.work_id}</code></dd>
    <dt>Edition id</dt>
    <dd><code>{row.id}</code></dd>
    {#if row.group_id}
      <dt>Group</dt>
      <dd><code>{row.group_id}</code></dd>
    {/if}
    {#if row.published_date}
      <dt>Published</dt>
      <dd>{row.published_date}</dd>
    {/if}
    {#if row.format_id}
      <dt>Format</dt>
      <dd><code>{row.format_id}</code></dd>
    {/if}
    {#if row.language_id}
      <dt>Language</dt>
      <dd><code>{row.language_id}</code></dd>
    {/if}
    {#if row.description}
      <dt>Description</dt>
      <dd class="multiline">{row.description}</dd>
    {/if}
    {#if row.notes}
      <dt>Notes</dt>
      <dd class="multiline">{row.notes}</dd>
    {/if}
    <dt>Created</dt>
    <dd>{row.created_at}</dd>
    {#if row.updated_at}
      <dt>Updated</dt>
      <dd>{row.updated_at}</dd>
    {/if}
  </dl>
{/if}

<style>
  .edition-fields {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.5rem 1rem;
    margin: 0;
  }
  dt {
    font-weight: 600;
    color: var(--wa-color-text-quiet, currentColor);
    font-size: 0.8125rem;
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  dd.multiline {
    white-space: pre-wrap;
  }
  code {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.8125rem;
  }
</style>