<script lang="ts">
import { useQueryClient } from '@tanstack/svelte-query'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'svelte-sonner'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys } from '../query/keys'
import {
  type ImportBatchResult,
  type ImportRow,
  importFiles,
  listenToImportEvents,
  mergeBatchResult,
  reduceImportEvents,
} from './import'

interface Props {
  open: boolean
  onclose: () => void
}

let { open, onclose }: Props = $props()

const queryClient = useQueryClient()
const FILE_FILTERS = [{ name: 'Books', extensions: ['epub', 'pdf'] }]

let rows = $state<ImportRow[]>([])
let summary = $state<ImportBatchResult | null>(null)
let inflight = $state(0)

const busy = $derived(inflight > 0)
const failedCount = $derived(rows.filter((row) => row.status === 'failed').length)

function summarize(result: ImportBatchResult) {
  return `${result.imported} imported · ${result.duplicated} already in library · ${result.failed} failed`
}

let queue: Promise<void> = Promise.resolve()

function run(paths: readonly string[]): Promise<void> {
  const list = [...new Set(paths.filter((path) => path.length > 0))]
  if (list.length === 0) return Promise.resolve()
  for (const path of list) {
    if (!rows.some((row) => row.path === path)) rows = [...rows, { path, status: 'pending' }]
  }
  inflight += 1
  const job = queue
    .then(() => runBatch(list))
    .finally(() => {
      inflight -= 1
    })
  queue = job
  return job
}

async function runBatch(list: readonly string[]) {
  try {
    const result = await importFiles(list)
    rows = mergeBatchResult(rows, result)
    summary = result
    if (result.imported + result.duplicated > 0) {
      await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    }
    if (result.failed > 0) toast.warning(summarize(result))
    else toast.success(summarize(result))
  } catch (error) {
    const message = error instanceof Error ? error.message : 'Import failed'
    toast.error(message)
    const affected = new Set(list)
    rows = rows.map((row) =>
      affected.has(row.path) && row.status !== 'imported' && row.status !== 'duplicate'
        ? { ...row, status: 'failed' as const, message }
        : row,
    )
  }
}

async function chooseFiles() {
  try {
    const picked = await openDialog({ multiple: true, filters: FILE_FILTERS })
    if (!picked) return
    await run(Array.isArray(picked) ? picked : [picked])
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not open the file picker')
  }
}

function retryFailed() {
  void run(rows.filter((row) => row.status === 'failed').map((row) => row.path))
}

function clearRows() {
  rows = []
}

$effect(() => {
  if (!open) return
  let disposed = false
  let unlistenDrop: (() => void) | undefined
  let unlistenImport: (() => void) | undefined

  getCurrentWebviewWindow()
    .onDragDropEvent((event) => {
      if (event.payload.type === 'drop') void run(event.payload.paths)
    })
    .then((unlisten) => {
      if (disposed) unlisten()
      else unlistenDrop = unlisten
    })
    .catch(() => undefined)

  listenToImportEvents({
    batch: (payload) => {
      if (payload.phase === 'started') summary = null
      else if (payload.result) summary = payload.result
    },
    file: (payload) => {
      rows = reduceImportEvents(rows, payload)
    },
  })
    .then((unlisten) => {
      if (disposed) unlisten()
      else unlistenImport = unlisten
    })
    .catch(() => undefined)

  return () => {
    disposed = true
    unlistenDrop?.()
    unlistenImport?.()
  }
})
</script>

<wa-drawer label="Add a book" placement="end" open={open} onwa-after-hide={onclose}>
  <div class="body">
    {#if summary}
      <div class="summary">{summarize(summary)}</div>
    {/if}

    <div class="drop-zone">
      <wa-icon name="file-arrow-up"></wa-icon>
      <p>Drag EPUB or PDF files here</p>
      <ActionButton variant="brand" onclick={chooseFiles} disabled={busy}>Choose files…</ActionButton>
    </div>

    {#if rows.length > 0}
      <ul class="rows">
        {#each rows as row (row.path)}
          <li class="row" data-status={row.status}>
            <span class="row-name">{row.path.split(/[/\\]/).pop()}</span>
            <span class="row-status">
              {#if row.status === 'importing'}Importing…
              {:else if row.status === 'imported'}Imported
              {:else if row.status === 'duplicate'}Already in library
              {:else if row.status === 'failed'}{row.message ?? 'Failed'}
              {:else}Pending{/if}
            </span>
          </li>
        {/each}
      </ul>
      <div class="actions">
        {#if failedCount > 0}
          <ActionButton onclick={retryFailed} disabled={busy}>Retry failed ({failedCount})</ActionButton>
        {/if}
        <ActionButton onclick={clearRows} disabled={busy}>Clear</ActionButton>
      </div>
    {/if}
  </div>
</wa-drawer>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .summary {
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .drop-zone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xl);
    text-align: center;
    border: 0.0625rem dashed var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-alt);
  }

  .drop-zone p {
    margin: 0;
    color: var(--wa-color-text-secondary);
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .row:first-of-type {
    border-top: none;
  }

  .row-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-status {
    flex-shrink: 0;
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .row[data-status='failed'] .row-status {
    color: var(--wa-color-danger);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-s);
  }
</style>
