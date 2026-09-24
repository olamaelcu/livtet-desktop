<script lang="ts">
interface Props {
  open: boolean
  title: string
  message: string
  confirmLabel?: string
  onconfirm: () => void
  oncancel: () => void
}

let { open, title, message, confirmLabel = 'Confirm', onconfirm, oncancel }: Props = $props()
</script>

<wa-dialog label={title} open={open} onwa-after-hide={oncancel}>
  <div class="body">
    <p>{message}</p>
    <div class="actions">
      <button type="button" class="cancel" onclick={oncancel}>Cancel</button>
      <button type="button" class="danger" onclick={onconfirm}>{confirmLabel}</button>
    </div>
  </div>
</wa-dialog>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-s);
  }

  .actions button {
    padding: var(--wa-space-2xs) var(--wa-space-m);
    border-radius: var(--wa-radius-m);
    border: 1px solid var(--wa-color-border-default);
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-default);
    cursor: pointer;
    font: inherit;
  }

  .actions button:hover {
    background: var(--wa-color-surface-alt);
  }

  .danger {
    color: var(--wa-color-danger);
    border-color: var(--wa-color-danger);
    font-weight: 600;
  }
</style>
