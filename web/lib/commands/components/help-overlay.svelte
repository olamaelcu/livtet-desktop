<script lang="ts">
  import {
    formatForDisplay,
    getHotkeyRegistrations,
  } from "@tanstack/svelte-hotkeys";
  import { useCommandRegistry } from "../registry.svelte";
  import { defaultBindings } from "../defaults";
  import { helpState } from "../dialog-state.svelte";
  import type { Command } from "../types";

  const registry = useCommandRegistry();
  const registrations = getHotkeyRegistrations();

  const grouped = $derived.by(() => {
    const out = new Map<string, Command[]>();
    for (const c of registry.all()) {
      const list = out.get(c.category) ?? [];
      list.push(c);
      out.set(c.category, list);
    }
    return Array.from(out.entries());
  });

  function bindingFor(c: Command): string {
    const reg = registrations.hotkeys.find((r) => r.id === c.id);
    if (reg) return reg.hotkey;
    const seq = registrations.sequences.find((r) => r.id === c.id);
    if (seq) return seq.sequence.join(" ");
    const fallback = defaultBindings[c.id];
    if (Array.isArray(fallback)) return fallback.join(" ");
    return fallback ?? "";
  }
</script>

<wa-dialog
  open={helpState.open}
  label="Keyboard shortcuts"
  light-dismiss
  onwa-after-hide={() => {
    helpState.open = false;
  }}
>
  <div class="cheatsheet">
    {#each grouped as [category, cmds] (category)}
      <section>
        <h3>{category}</h3>
        <dl>
          {#each cmds as c (c.id)}
            <div class="row">
              <dt>{c.label}</dt>
              <dd>{formatForDisplay(bindingFor(c))}</dd>
            </div>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
</wa-dialog>

<style>
  wa-dialog::part(panel) {
    width: min(36rem, 90vw);
  }

  .cheatsheet {
    max-height: 60vh;
    overflow-y: auto;
  }

  h3 {
    margin: 1rem 0 0.25rem;
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--wa-color-text-quiet, currentColor);
  }

  h3:first-child {
    margin-top: 0;
  }

  dl {
    margin: 0;
  }

  .row {
    display: flex;
    justify-content: space-between;
    padding: 0.375rem 0;
    border-bottom: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.06));
  }

  .row:last-child {
    border-bottom: 0;
  }

  dt {
    margin: 0;
  }

  dd {
    margin: 0;
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.8125rem;
    color: var(--wa-color-text-quiet, currentColor);
  }
</style>