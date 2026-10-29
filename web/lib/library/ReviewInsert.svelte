<script lang="ts">
  interface WaTextarea {
    value: string;
  }

  interface WaRating {
    value: number;
  }

  interface Props {
    onSuccess?: () => void;
    cancel?: () => void;
    onSubmit?: () => void;
    onClose?: () => void;
    form?: { ok?: boolean; error?: string };
  }

  let { onSuccess, cancel, onSubmit, onClose, form }: Props = $props();
  let reviewBody = $state("");
  let rating = $state<number>(0);
  let submitting = $state(false);
  let canSubmit = $derived.by(() => {
    return reviewBody.trim().length > 0 && !submitting;
  });

  $effect(() => {
    if (form?.ok) {
      reviewBody = "";
      rating = 0;
      submitting = false;
      onSuccess?.();
    }
    if (form?.error) {
      submitting = false;
    }
  });

  function submitReview(e: Event) {
    e.preventDefault();
    if (!reviewBody.trim()) return;
    submitting = true;
    onSubmit?.();
    onClose?.();
  }
</script>

{#if form?.ok}
  <wa-callout variant="success">Thanks — your review was published.</wa-callout>
{/if}
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
    oninput={(e: Event) => {
      const next = Number((e.currentTarget as unknown as WaRating).value ?? 0);
      rating = Number.isFinite(next) ? next : 0;
    }}
  ></wa-rating>
  {#if rating > 0}
    <input type="hidden" name="rating" value={String(rating)} />
  {/if}
  <div>
    <p>
      By making a review, you understand and accept <a href="#" class="wa-link">the review guidelines</a>.
    </p>
    <wa-button-group>
      {#if cancel}
        <wa-button variant="text" onclick={cancel}>Cancel</wa-button>
      {:else}
        <wa-button variant="text" onclick={() => onClose?.()}>Cancel</wa-button>
      {/if}
      <wa-button
        type="submit"
        class="wa-success"
        disabled={!canSubmit}
        loading={submitting}>Submit</wa-button>
    </wa-button-group>
  </div>
</form>

<style>
  wa-textarea,
  wa-rating {
    margin-bottom: var(--wa-space-s);
  }
  div {
    display: flex;
    flex-direction: row;
    align-items: center;

    p {
      font-size: var(--wa-font-size-s);
      color: var(--wa-color-text-quiet);
      flex: 1 1;
    }

    button[type="submit"] {
      align-self: flex-end;
      max-width: fit-content;
      width: fit-content;
    }
  }
  form {
    margin: var(--wa-space-s) 0;
    display: flex;
    flex-direction: column;
  }
  .wa-link {
    color: var(--wa-color-accent);
    text-decoration: underline;
  }
</style>