<script lang="ts">
import { commands, type DigitalInventoryRow } from '$lib/bindings'
import FileRow from './file-row.svelte'

interface Props {
  editionId: string
}

let { editionId }: Props = $props()

let file = $state<DigitalInventoryRow | null>(null)
let loading = $state(true)
let error = $state<string | null>(null)

$effect(() => {
  let cancelled = false
  loading = true
  error = null
  file = null
  commands
    .findFilesByEdition(editionId)
    .then((res) => {
      if (cancelled) return
      if (res.status === 'ok') {
        file = res.data
        error = null
      } else {
        error = res.error
        file = null
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
    Loading files…
  </wa-callout>
{:else if error}
  <wa-callout variant="danger">
    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
    Failed to load: {error}
  </wa-callout>
{:else if !file}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="circle-info"></wa-icon>
    No files linked to this edition yet.
  </wa-callout>
{:else}
  <FileRow {file} />
{/if}