import { toast } from 'svelte-sonner'
import { ulid } from 'ulid'
import { commands, events, type ProviderFailureEvent, type SearchHitRow } from '$lib/bindings'
import { FAILURE_TOAST } from './types'

let currentRequestId: string | null = null
let unlistenPromise: Promise<() => void> | null = null

export async function subscribeProviderFailures(): Promise<() => void> {
  if (unlistenPromise) return unlistenPromise
  unlistenPromise = events.providerFailureEvent.listen((e) => {
    if (e.payload.request_id !== currentRequestId) return
    toast.error(FAILURE_TOAST(e.payload.provider))
  })
  return unlistenPromise
}

export async function runSearch(
  query: string,
  limit: number,
  onResults: (hits: SearchHitRow[]) => void,
): Promise<void> {
  if (currentRequestId !== null) {
    await commands.cancelRemoteSearch(currentRequestId)
  }
  const id = ulid()
  currentRequestId = id

  const res = await commands.remoteSearch(query, limit, id)
  if (id !== currentRequestId) return
  if (res.status === 'ok') onResults(res.data.results)
  else onResults([])
}
