<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import { syncKeys } from '../query/keys'
import {
  listenToSyncEvents,
  syncErrorMessage,
  syncOfferCover,
  syncOffersDecide,
  syncOffersList,
} from '../sync'
import ActionButton from './ActionButton.svelte'

/**
 * Asks the reader about a book a paired device sent.
 *
 * The daemon records an offer on its pull tick and waits: nothing is on disk,
 * so the title, authors, and cover here all come from the sender. Answering is
 * what downloads the book, or refuses it for good.
 */

const queryClient = useQueryClient()

// Offers arrive on the daemon's schedule, so poll as a floor and let the
// `sync://offer-received` event close the gap between a pull and the prompt.
const offers = createQuery(() => ({
  queryKey: syncKeys.offers(),
  queryFn: syncOffersList,
  refetchInterval: 30_000,
}))

/** The daemon lists newest first; the reader gets the oldest pending offer. */
const offer = $derived(offers.data?.at(-1) ?? null)

const cover = createQuery(() => ({
  queryKey: syncKeys.offerCover(offer?.offer_id ?? ''),
  queryFn: () => syncOfferCover(offer?.offer_id ?? ''),
  enabled: offer?.has_cover === true,
  // A cover is fetched from the sender once per offer and never changes.
  staleTime: Number.POSITIVE_INFINITY,
  retry: false,
}))

const decide = createMutation(() => ({
  mutationFn: (variables: { offerId: string; title: string; accepted: boolean }) =>
    syncOffersDecide(variables.offerId, variables.accepted),
  onSuccess: (result, variables) => {
    toast.success(result.accepted ? `Added ${variables.title}` : `Declined ${variables.title}`)
    if (!result.delivered) {
      toast.warning('The sending device could not be reached; it will not know yet.')
    }
    // Accepting writes a book and its catalog rows, so the whole cache is
    // stale; declining only removes the offer.
    void queryClient.invalidateQueries()
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

/**
 * Re-mounting the dialog is how it comes back after being closed: an offer
 * that has not been answered is still pending, and the reader has no other
 * place to find it.
 */
let shows = $state(0)

onMount(() => {
  let unlisten: (() => void) | undefined
  let cancelled = false
  void listenToSyncEvents({
    'sync://offer-received': () => {
      void queryClient.invalidateQueries({ queryKey: syncKeys.offers() })
    },
    'sync://offer-decided': (payload) => {
      const decided = payload as { accepted?: boolean } | null
      toast.success(
        decided?.accepted
          ? 'The other device accepted the book'
          : 'The other device declined the book',
      )
      void queryClient.invalidateQueries({ queryKey: syncKeys.offers() })
    },
  })
    .then((stop) => {
      if (cancelled) stop()
      else unlisten = stop
    })
    // Outside Tauri (SSR, component tests) there is no event bridge; polling
    // still surfaces the offer.
    .catch(() => {})

  return () => {
    cancelled = true
    unlisten?.()
  }
})

function answer(accepted: boolean) {
  if (!offer) return
  decide.mutate({ offerId: offer.offer_id, title: offer.title, accepted })
}
</script>

{#if offer}
  {#key shows}
    <wa-dialog label="Book offered" open onwa-after-hide={() => (shows += 1)}>
      <div class="body">
        <div class="book">
          {#if cover.data?.data_url}
            <img class="cover" src={cover.data.data_url} alt="" />
          {:else}
            <div class="cover placeholder" aria-hidden="true">📖</div>
          {/if}
          <div class="details">
            <strong class="title">{offer.title}</strong>
            <span class="authors">{offer.authors.join(', ') || 'Unknown author'}</span>
            <span class="muted">from {offer.from_name ?? offer.from_device_id}</span>
          </div>
        </div>

        <p class="muted">Accepting downloads the book into your library.</p>

        <div class="actions">
          <ActionButton
            variant="brand"
            onclick={() => answer(true)}
            disabled={decide.isPending}
          >
            Accept
          </ActionButton>
          <ActionButton onclick={() => answer(false)} disabled={decide.isPending}>
            Decline
          </ActionButton>
        </div>
      </div>
    </wa-dialog>
  {/key}
{/if}

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    max-width: 28rem;
  }

  .book {
    display: flex;
    gap: var(--wa-space-m);
    align-items: flex-start;
  }

  .cover {
    width: 6rem;
    height: 9rem;
    border-radius: var(--wa-radius-m);
    object-fit: cover;
    background: var(--wa-color-surface-lowered);
  }

  .placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 2rem;
  }

  .details {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    min-width: 0;
  }

  .title {
    font-size: var(--wa-font-size-l);
  }

  .authors,
  .muted {
    color: var(--wa-color-text-quiet);
  }

  .actions {
    display: flex;
    gap: var(--wa-space-s);
    justify-content: flex-end;
  }
</style>
