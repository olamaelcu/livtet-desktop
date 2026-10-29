<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { commands, type LocalReview } from '../bindings'
import { catalogKeys } from '../query/keys'
import { formatNoteDate } from '../reader/progress'
import ReviewInsert from './ReviewInsert.svelte'

interface Props {
  editionId: string
  /** Whether the listing itself is on screen. */
  open: boolean
  /**
   * Whether the book's detail drawer is open. The query follows this, not
   * `open`: the trigger button shows how many reviews are stored, so the
   * count has to be known before the listing is ever opened.
   */
  enabled: boolean
  onclose: () => void
  /** How many reviews are stored, so the caller's trigger can show it. */
  count?: number
  /** Book title, needed to resolve the edition when republishing an edit. */
  bookTitle?: string
  /** Whether the account can publish; threads into the edit form's switch. */
  signedIn?: boolean
}

let {
  editionId,
  open,
  enabled,
  onclose,
  count = $bindable(0),
  bookTitle = '',
  signedIn,
}: Props = $props()

const queryClient = useQueryClient()

let editing = $state<LocalReview | null>(null)
let editForm = $state<{ ok?: boolean; error?: string }>()
let editSubmitting = $state(false)

/**
 * The reviews this desktop stored for the edition. A second query, like the
 * reader notes, because they live in their own table rather than the catalog
 * record.
 */
const localReviews = createQuery(() => ({
  queryKey: catalogKeys.editionReviews(editionId),
  queryFn: async () => {
    const result = await commands.loadLocalReviews(editionId)
    if (result.status === 'error') throw new Error(result.error.message)
    return result.data
  },
  enabled: enabled && editionId !== '',
}))

$effect(() => {
  count = localReviews.data?.length ?? 0
})

/**
 * Delete a stored review.
 *
 * The local row is the only thing removed: a published copy lives in the
 * account's repository and is left alone, which the toast says out loud when
 * one exists. `false` means the row was already gone, which is not an error.
 */
async function removeReview(review: LocalReview) {
  const result = await commands.removeLocalReview(review.id)
  if (result.status === 'error') {
    toast.error(`Could not delete the review: ${result.error.message}`)
    return
  }
  void queryClient.invalidateQueries({ queryKey: catalogKeys.editionReviews(editionId) })
  toast.success(
    review.record_uri
      ? 'Review deleted here — the copy on your PDS is unchanged'
      : 'Review deleted',
  )
}

function startEdit(review: LocalReview) {
  editForm = undefined
  editing = review
}

function cancelEdit() {
  editing = null
}

/**
 * Save the edited review and upsert its remote record.
 *
 * The table holds one review per book, so saving updates the same row; the
 * returned id is the handle publishing uses to overwrite the same record.
 */
async function submitEdit(text: string, rating: number, publish: boolean) {
  if (!editing || editSubmitting) return
  const stars = rating > 0 ? rating : null
  editSubmitting = true
  try {
    const saved = await commands.saveLocalReview(editionId, text, stars)
    if (saved.status === 'error') {
      editForm = { error: saved.error.message }
      return
    }
    editForm = { ok: true }
    editing = null
    await queryClient.invalidateQueries({ queryKey: catalogKeys.editionReviews(editionId) })
    if (!publish) {
      toast.success('Review updated')
      return
    }
    const sync = await commands.putLocalReviewToPds(saved.data.id, bookTitle)
    if (sync.status === 'error') {
      toast.error(`Review updated — publishing failed: ${sync.error.message}`)
    } else if (sync.data.error) {
      toast.error(`Review updated — publishing failed: ${sync.data.error}`)
    } else {
      toast.success('Review updated and published to your PDS')
    }
  } catch (error) {
    editForm = {
      error: error instanceof Error ? error.message : 'Could not update the review',
    }
  } finally {
    editSubmitting = false
  }
}
</script>

<wa-drawer label="Reviews" placement="end" {open} onwa-after-hide={onclose}>
  <div class="reviews">
    {#if localReviews.isPending}
      <p class="muted">Loading…</p>
    {:else if localReviews.isError}
      <p class="error">Could not load this book's reviews.</p>
    {:else if !localReviews.data || localReviews.data.length === 0}
      <p class="muted">No reviews saved for this book yet.</p>
    {:else}
      <ul class="review-list">
        {#each localReviews.data ?? [] as review (review.id)}
          <li class="review">
            <div class="review-head">
              {#if review.rating !== null}
                <p class="review-rating">{review.rating}/10</p>
              {/if}
              {#if review.record_uri}
                <wa-badge title={review.record_uri}>Published</wa-badge>
              {/if}
              <div class="review-actions">
                <!-- `wa-button` keeps its semantics in shadow DOM, which
                     Svelte's a11y checks cannot see; same suppression as ActionButton. -->
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <wa-button
                  class="review-edit"
                  size="small"
                  title="Edit this review"
                  onclick={() => startEdit(review)}
                >
                  <wa-icon name="pen"></wa-icon>
                </wa-button>
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <wa-button
                  class="review-delete"
                  size="small"
                  title="Delete this review"
                  onclick={() => removeReview(review)}
                >
                  <wa-icon name="trash"></wa-icon>
                </wa-button>
              </div>
            </div>
            <p class="review-text">{review.text}</p>
            {#if formatNoteDate(review.created_at)}
              <time class="review-date" datetime={review.created_at}>
                {formatNoteDate(review.created_at)}
              </time>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</wa-drawer>

<wa-dialog
  open={editing !== null}
  label="Edit review"
  onwa-after-hide={() => (editing = null)}
>
  {#if editing}
    <ReviewInsert
      form={editForm}
      {signedIn}
      initialText={editing.text}
      initialRating={editing.rating ?? 0}
      onSubmit={submitEdit}
      onClose={cancelEdit}
    />
  {/if}
</wa-dialog>

<style>
  .reviews {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .review-list {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .review {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    padding-block-start: var(--wa-space-s);
    border-block-start: 1px solid var(--wa-color-border-default);
  }

  .review-head {
    display: flex;
    align-items: center;
    gap: var(--wa-space-2xs);
  }

  .review-rating {
    margin: 0;
    font-weight: var(--wa-font-weight-semibold, 600);
  }

  .review-actions {
    margin-inline-start: auto;
    display: flex;
    gap: var(--wa-space-3xs);
  }

  .review-text {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .review-date {
    font-size: var(--wa-font-size-2xs, 0.75rem);
    color: var(--wa-color-text-muted);
  }

  .muted {
    color: var(--wa-color-text-secondary);
  }

  .error {
    color: var(--wa-color-danger);
  }
</style>
