<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys } from '../query/keys'
import { loadFilterOptions } from '../search'

interface Props {
  onadd: (tag: string) => void
  onremove: (tagId: string) => void
  onclose: () => void
}

let { onadd, onremove, onclose }: Props = $props()

const options = createQuery(() => ({
  queryKey: searchKeys.filterOptions(),
  queryFn: loadFilterOptions,
  staleTime: 5 * 60 * 1000,
}))

let draft = $state('')

function handleInput(event: Event) {
  draft = (event.target as HTMLInputElement).value
}

function submit() {
  const name = draft.trim()
  if (!name) return
  onadd(name)
  draft = ''
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault()
    submit()
  }
}
</script>

<div class="picker">
  <div class="create">
    <!--
      `wa-input` is a WebAwesome custom element: its interactive semantics live
      in its shadow DOM, which Svelte's static a11y checks cannot see.
    -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <wa-input
      placeholder="New tag…"
      value={draft}
      oninput={handleInput}
      onkeydown={handleKeydown}
    ></wa-input>
    <ActionButton variant="brand" onclick={submit} disabled={draft.trim().length === 0}>
      Add
    </ActionButton>
  </div>
  <ul>
    {#each options.data?.tags ?? [] as tag (tag.id)}
      <li>
        <span>{tag.label}</span>
        <button
          type="button"
          class="remove"
          aria-label={`Remove ${tag.label} from the selected editions`}
          onclick={() => onremove(tag.id)}
        >
          <wa-icon name="xmark"></wa-icon>
        </button>
      </li>
    {:else}
      <li class="empty">No tags yet.</li>
    {/each}
  </ul>
  <div class="footer">
    <ActionButton onclick={onclose}>Done</ActionButton>
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    min-width: 16rem;
  }

  .create {
    display: flex;
    gap: var(--wa-space-s);
    align-items: flex-end;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    max-height: 16rem;
    overflow-y: auto;
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
  }

  li.empty {
    color: var(--wa-color-text-secondary);
  }

  .remove {
    display: inline-flex;
    padding: var(--wa-space-3xs);
    border: none;
    background: none;
    color: var(--wa-color-text-secondary);
    cursor: pointer;
    border-radius: var(--wa-radius-m);
  }

  .remove:hover {
    color: var(--wa-color-danger);
    background: var(--wa-color-surface-alt);
  }

  .footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
