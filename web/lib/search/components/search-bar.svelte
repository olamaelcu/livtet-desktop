<script lang="ts">
  interface Props {
    value: string;
    placeholder?: string;
  }

  let { value = $bindable(""), placeholder = "Search the library…" }: Props =
    $props();

  // TS-only cast: the runtime target is the <wa-input> custom element.
  function oninput(e: Event) {
    const target = e.target as HTMLInputElement | null;
    if (target) value = target.value;
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
    <wa-icon
      slot="end"
      name="xmark"
      class="clear-button"
      role="button"
      tabindex="0"
      aria-label="Clear search"
      onclick={() => (value = "")}
      onkeydown={(e: KeyboardEvent) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          value = "";
        }
      }}
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
