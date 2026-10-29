<script lang="ts">
interface WaTextarea {
  value: string
}

interface WaRating {
  value: number
}

interface WaSwitch {
  checked: boolean
}

interface Props {
  /** Set by the caller after a submit: `ok` clears the form, `error` shows why. */
  form?: { ok?: boolean; error?: string }
  /**
   * Whether the account can publish at all. `false` locks the switch off and
   * says why; `undefined` means "not known yet" and leaves the default alone.
   */
  signedIn?: boolean
  /** Existing review text, when editing the edition's one review. */
  initialText?: string
  /** Existing rating (`0` = unrated), when editing. */
  initialRating?: number
  /** Called with the trimmed text, the rating (`0` = unrated), and the switch. */
  onSubmit?: (text: string, rating: number, publish: boolean) => void
  onClose?: () => void
}

let { form, signedIn, initialText = '', initialRating = 0, onSubmit, onClose }: Props = $props()
let reviewBody = $state('')
let rating = $state<number>(0)
let submitting = $state(false)
// Publishing is the point of the feature, so it starts on — but a signed-out
// account cannot publish, and a switch that is on and always fails is a lie.
let publish = $state(true)
const canPublish = $derived(signedIn !== false)
const canSubmit = $derived(reviewBody.trim().length > 0 && !submitting)

// Prefill from the caller's review whenever it changes (e.g. the stored review
// loads after the dialog mounts). Text and rating are primitives, so this only
// runs on a real change, never per keystroke.
$effect(() => {
  reviewBody = initialText
  rating = initialRating
})

$effect(() => {
  if (!canPublish) publish = false
})

$effect(() => {
  if (form?.ok) {
    reviewBody = ''
    rating = 0
    submitting = false
    publish = canPublish
  }
  if (form?.error) {
    submitting = false
  }
})

function submitReview(event: SubmitEvent) {
  event.preventDefault()
  if (!reviewBody.trim() || submitting) return
  submitting = true
  onSubmit?.(reviewBody.trim(), rating, publish && canPublish)
}
</script>

{#if form?.error}
  <wa-callout variant="danger">{form.error}</wa-callout>
{/if}

<form onsubmit={submitReview}>
  <wa-textarea
    name="text"
    label="Review text"
    placeholder="What did you think of this book?"
    required
    value={reviewBody}
    oninput={(e: Event) =>
      (reviewBody = (e.currentTarget as unknown as WaTextarea).value)}
  ></wa-textarea>
  <wa-rating
    label="Rating (optional)"
    precision={0.5}
    min={0}
    max={10}
    value={rating}
    onchange={(e: Event) => {
      const next = Number((e.currentTarget as unknown as WaRating).value ?? 0)
      rating = Number.isFinite(next) ? next : 0
    }}
  ></wa-rating>
  <div class="publish">
    <wa-switch
      checked={publish && canPublish}
      disabled={!canPublish}
      onchange={(event: Event) =>
        (publish = (event.currentTarget as unknown as WaSwitch).checked)}
    >
      Publish to my ATProto account
    </wa-switch>
    <p class="publish-hint">
      {#if canPublish}
        Your review is kept here either way; publishing also posts it to your account.
      {:else}
        Sign in on Settings to publish reviews to your account.
      {/if}
    </p>
  </div>
  <div class="actions">
    {#if rating > 0}
      <span class="rating-note">{rating}/10</span>
    {/if}
    <wa-button-group>
      <!-- `wa-button` keeps its semantics in shadow DOM, which Svelte's a11y
           checks cannot see; the same suppression as ActionButton. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <wa-button variant="text" onclick={() => onClose?.()}>Cancel</wa-button>
      <wa-button type="submit" variant="brand" disabled={!canSubmit} loading={submitting}>
        Submit
      </wa-button>
    </wa-button-group>
  </div>
</form>

<style>
  wa-textarea,
  wa-rating {
    margin-bottom: var(--wa-space-s);
  }

  .publish {
    margin-bottom: var(--wa-space-s);
  }

  .publish-hint {
    margin: var(--wa-space-3xs) 0 0;
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    justify-content: flex-end;
  }

  .rating-note {
    margin-inline-end: auto;
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-s);
  }

  form {
    margin: var(--wa-space-s) 0;
    display: flex;
    flex-direction: column;
  }
</style>
