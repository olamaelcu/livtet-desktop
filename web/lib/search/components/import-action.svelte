<script lang="ts">
import { toast } from 'svelte-sonner'
import { commands, type ImportRequest } from '$lib/bindings'
import { openPeek } from '$lib/catalog/peek-state.svelte'
import type { SearchHit } from '../types'

interface Props {
  hit: SearchHit
}

let { hit }: Props = $props()

let importing = $state(false)

async function onImport(e: Event): Promise<void> {
  e.stopPropagation()
  if (importing) return
  importing = true
  try {
    const request: ImportRequest = {
      title: hit.title,
      authors: hit.authors,
      isbn: hit.isbn,
      isbn_13: hit.isbn_13,
      publisher: hit.publisher,
      page_count: hit.page_count,
      language: hit.language,
      published_date: hit.published_date,
      description: hit.description,
      provider: hit.source,
      provider_work_id: hit.work_id,
      provider_edition_url: null,
    }
    const res = await commands.importEdition(request)
    if (res.status === 'error') {
      toast.error(res.error)
    } else if (res.data === 'AlreadyExists') {
      toast.warning('Already in your catalog')
    } else {
      toast.success(`Imported "${request.title}" to your catalog`)
      openPeek(res.data.Created.edition_id)
    }
  } catch (e) {
    toast.error(e instanceof Error ? e.message : 'Import failed')
  } finally {
    importing = false
  }
}
</script>

{#if !hit.edition_id}
  <wa-button
    size="s"
    appearance="outlined"
    disabled={importing}
    onclick={onImport}
  >
    {importing ? "…" : "Import"}
  </wa-button>
{/if}
