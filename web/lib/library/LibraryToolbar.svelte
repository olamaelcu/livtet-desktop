<script lang="ts">
import ActionButton from '../components/ActionButton.svelte'

interface Props {
  onaddbook: () => void
}

let { onaddbook }: Props = $props()
</script>

<div class="toolbar" role="toolbar" aria-label="Library actions">
  <wa-button-group>
    <ActionButton variant="brand" onclick={onaddbook}>
      <wa-icon name="plus"></wa-icon>
      Add book
    </ActionButton>
  </wa-button-group>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
    padding: var(--wa-space-xs) var(--wa-space-xs) 0;
  }
</style>
