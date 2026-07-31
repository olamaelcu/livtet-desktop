<script lang="ts">
import { attachAsButton } from '$lib/a11y/attachments'

interface Props {
  value: string
  placeholder?: string
}

let { value = $bindable(''), placeholder = 'Search the library…' }: Props = $props()

// TS-only cast: the runtime target is the <wa-input> custom element.
function oninput(e: Event) {
  const target = e.target as HTMLInputElement | null
  if (target) value = target.value
}
</script>

<wa-input
  class="search-bar"
  {placeholder}
  value={value || ""}
  oninput={oninput}
  type="search"
>
  <wa-icon slot="start" name="magnifying-glass"></wa-icon>
  {#if value}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!--   role="button" + tabindex="0" are inline so Svelte re-applies them on
         every render of the {#if value} block; the runtime keydown listener
         is added by attachAsButton (attachActivate) and Svelte's analyzer
         can't see it. -->
    <wa-icon
      slot="end"
      name="xmark"
      class="clear-button"
      role="button"
      tabindex="0"
      aria-label="Clear search"
      onclick={() => (value = "")}
      {@attach attachAsButton}
    ></wa-icon>
  {/if}
</wa-input>

<style>
  .search-bar {
    width: 100%;
  }

  .clear-button {
    cursor: pointer;
    padding: 0.25rem;
    border-radius: var(--wa-border-radius-s, 4px);
  }

  .clear-button:hover {
    background: var(--wa-color-neutral-fill-hover, rgba(0, 0, 0, 0.05));
  }

  .clear-button:focus-visible {
    outline: 2px solid var(--wa-color-focus, currentColor);
    outline-offset: 2px;
  }
</style>
